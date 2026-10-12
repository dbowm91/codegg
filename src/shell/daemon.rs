//! Daemon-owned finite human shell runs.

use sha2::Digest;
use std::collections::{HashMap, VecDeque};
use std::sync::Arc;
use std::time::Duration;

use codegg_protocol::core::CoreResponse;
use codegg_protocol::human_shell::{
    HumanShellOutputChunkDto, HumanShellRunCreateRequestDto, HumanShellRunMetadataDto,
    HumanShellRunStatusDto, HUMAN_SHELL_MAX_COMMAND_BYTES, HUMAN_SHELL_MAX_READ_BYTES,
    HUMAN_SHELL_PROTOCOL_VERSION,
};
use parking_lot::Mutex;
use tokio::sync::mpsc;

const RUN_TTL_MS: i64 = 10 * 60 * 1000;
const MAX_RUNS_PER_PRINCIPAL: usize = 8;
const MAX_RUNS_TOTAL: usize = 512;
const MAX_OUTPUT_BYTES: usize = 1_000_000;
const MAX_RETAINED_CHUNKS: usize = 512;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HumanShellRunScope {
    pub principal_id: String,
    pub project_id: String,
    pub workspace_id: String,
    pub session_id: Option<String>,
}

#[derive(Debug, Clone)]
pub struct RemoteShellRunBinding {
    pub run_id: String,
    pub project_id: String,
    pub workspace_id: String,
    pub session_id: Option<String>,
}

#[derive(Debug)]
struct RunRecord {
    meta: HumanShellRunMetadataDto,
    scope: HumanShellRunScope,
    submission_id: String,
    command_digest: [u8; 32],
    next_sequence: u64,
    chunks: VecDeque<HumanShellOutputChunkDto>,
    retained_bytes: usize,
    output_truncated: bool,
    handle: Option<super::runtime::ShellHandle>,
}

#[derive(Default)]
struct RegistryInner {
    runs: HashMap<String, Arc<Mutex<RunRecord>>>,
    submissions: HashMap<(String, String), String>,
}

/// Ephemeral run authority and bounded output retention. Runs and handles
/// disappear on daemon restart; no command is replayed.
pub struct HumanShellRunService {
    inner: tokio::sync::Mutex<RegistryInner>,
    admission: Arc<crate::scheduler::admission::AdmissionController>,
}

impl HumanShellRunService {
    pub fn new(admission: Arc<crate::scheduler::admission::AdmissionController>) -> Self {
        Self {
            inner: tokio::sync::Mutex::new(RegistryInner::default()),
            admission,
        }
    }

    pub async fn create(
        &self,
        scope: HumanShellRunScope,
        request: HumanShellRunCreateRequestDto,
        cwd: std::path::PathBuf,
        execution_node: String,
        shell: String,
        sandbox: Option<crate::security::sandbox::SandboxLaunchSpec>,
    ) -> CoreResponse {
        if request.protocol_version != HUMAN_SHELL_PROTOCOL_VERSION
            || request.submission_id.is_empty()
            || request.submission_id.len() > 128
            || request.command.is_empty()
            || request.command.len() > HUMAN_SHELL_MAX_COMMAND_BYTES
            || request.command.contains('\0')
            || request.timeout_secs == 0
            || request.timeout_secs > codegg_protocol::human_shell::HUMAN_SHELL_MAX_TIMEOUT_SECS
        {
            return error(
                "human_shell_invalid_request",
                "invalid or oversized human shell request",
            );
        }
        if request.project_id != scope.project_id
            || request.workspace_id != scope.workspace_id
            || request.session_id != scope.session_id
        {
            return error(
                "human_shell_scope_mismatch",
                "human shell scope is not authorized",
            );
        }

        let digest: [u8; 32] = sha2::Sha256::digest(request.command.as_bytes()).into();
        let mut inner = self.inner.lock().await;
        Self::prune(&mut inner);
        if let Some(run_id) = inner
            .submissions
            .get(&(scope.principal_id.clone(), request.submission_id.clone()))
        {
            if let Some(run) = inner.runs.get(run_id) {
                let run = run.lock();
                if run.scope == scope
                    && run.command_digest == digest
                    && run.meta.origin == request.origin
                {
                    return CoreResponse::HumanShellRunCreated {
                        run: run.meta.clone(),
                        duplicate: true,
                    };
                }
                return error(
                    "human_shell_submission_conflict",
                    "submission identity was already used for different content or scope",
                );
            }
            inner
                .submissions
                .remove(&(scope.principal_id.clone(), request.submission_id.clone()));
        }
        if inner
            .runs
            .values()
            .filter(|run| run.lock().scope.principal_id == scope.principal_id)
            .count()
            >= MAX_RUNS_PER_PRINCIPAL
        {
            return error(
                "human_shell_run_limit",
                "too many retained human shell runs for this principal",
            );
        }
        if inner.runs.len() >= MAX_RUNS_TOTAL {
            return error(
                "human_shell_capacity",
                "daemon human shell retention is full",
            );
        }
        let permit =
            match self
                .admission
                .try_admit_arc(&crate::scheduler::permit::PermitDimensions {
                    cpu_weight: 1,
                    process_slots: 1,
                    io_weight: 1,
                    ..Default::default()
                }) {
                crate::scheduler::admission::AdmissionDecision::Admitted(permit) => permit,
                crate::scheduler::admission::AdmissionDecision::TemporarilyBlocked(reason) => {
                    return error(
                        "human_shell_capacity",
                        &format!("human shell admission is busy: {reason:?}"),
                    );
                }
                crate::scheduler::admission::AdmissionDecision::Impossible(reason) => {
                    return error(
                        "human_shell_unavailable",
                        &format!("human shell admission is unavailable: {reason:?}"),
                    );
                }
            };

        let now = chrono::Utc::now().timestamp_millis();
        let run_id = uuid::Uuid::new_v4().to_string();
        let local_id = crate::shell::types::ShellCommandId(u64::from_be_bytes(
            uuid::Uuid::parse_str(&run_id)
                .expect("generated UUID")
                .as_bytes()[..8]
                .try_into()
                .expect("eight bytes"),
        ));
        let capture_policy =
            if request.origin == codegg_protocol::human_shell::HumanShellOriginDto::BangBang {
                crate::shell::types::ShellCapturePolicy::StoreAndPromote
            } else {
                crate::shell::types::ShellCapturePolicy::StoreEphemeral
            };
        let shell_request = crate::shell::types::ShellRequest {
            id: local_id,
            origin: if request.origin == codegg_protocol::human_shell::HumanShellOriginDto::BangBang
            {
                crate::shell::types::ShellOrigin::HumanPromoted
            } else {
                crate::shell::types::ShellOrigin::HumanEphemeral
            },
            command: request.command.clone(),
            cwd: cwd.clone(),
            timeout: Duration::from_secs(request.timeout_secs),
            capture_policy,
            env_policy: crate::shell::types::ShellEnvPolicy::Clean,
        };
        let (event_tx, mut event_rx) = mpsc::channel(128);
        let mut runtime = super::runtime::ShellRuntime::with_host_shell(shell);
        if let Some(spec) = sandbox {
            runtime = runtime.with_sandbox(spec);
        }
        let handle = match runtime.spawn(shell_request, event_tx).await {
            Ok(handle) => handle,
            Err(message) => return error("human_shell_start_failed", &message),
        };
        let meta = HumanShellRunMetadataDto {
            run_id: run_id.clone(),
            project_id: scope.project_id.clone(),
            workspace_id: scope.workspace_id.clone(),
            session_id: scope.session_id.clone(),
            execution_node,
            cwd: cwd.to_string_lossy().to_string(),
            command: request.command,
            origin: request.origin,
            status: HumanShellRunStatusDto::Running,
            exit_code: None,
            expires_at_ms: now.saturating_add(RUN_TTL_MS),
        };
        let record = Arc::new(Mutex::new(RunRecord {
            meta: meta.clone(),
            scope: scope.clone(),
            submission_id: request.submission_id.clone(),
            command_digest: digest,
            next_sequence: 1,
            chunks: VecDeque::new(),
            retained_bytes: 0,
            output_truncated: false,
            handle: Some(handle),
        }));
        inner.submissions.insert(
            (scope.principal_id.clone(), request.submission_id),
            run_id.clone(),
        );
        inner.runs.insert(run_id.clone(), record.clone());
        tokio::spawn(async move {
            let _permit = permit;
            while let Some(event) = event_rx.recv().await {
                let mut run = record.lock();
                match event {
                    crate::shell::ShellEvent::Stdout { bytes, .. } => {
                        append_output(&mut run, false, &bytes)
                    }
                    crate::shell::ShellEvent::Stderr { bytes, .. } => {
                        append_output(&mut run, true, &bytes)
                    }
                    crate::shell::ShellEvent::Exited { status, .. } => {
                        run.meta.status = if status.is_some() {
                            HumanShellRunStatusDto::Exited
                        } else {
                            HumanShellRunStatusDto::Cancelled
                        };
                        run.meta.exit_code = status;
                        run.handle = None;
                    }
                    crate::shell::ShellEvent::TimedOut { .. } => {
                        run.meta.status = HumanShellRunStatusDto::TimedOut;
                        run.handle = None;
                    }
                    crate::shell::ShellEvent::FailedToStart { .. } => {
                        run.meta.status = HumanShellRunStatusDto::Failed;
                        run.handle = None;
                    }
                    crate::shell::ShellEvent::Started { .. } => {}
                }
            }
            let mut run = record.lock();
            if run.meta.status == HumanShellRunStatusDto::Running {
                run.meta.status = HumanShellRunStatusDto::Lost;
                run.handle = None;
            }
        });
        CoreResponse::HumanShellRunCreated {
            run: meta,
            duplicate: false,
        }
    }

    pub async fn read(
        &self,
        scope: &HumanShellRunScope,
        run_id: &str,
        from_sequence: u64,
        max_bytes: usize,
    ) -> CoreResponse {
        if max_bytes == 0 || max_bytes > HUMAN_SHELL_MAX_READ_BYTES {
            return error(
                "human_shell_read_limit",
                "human shell read exceeds its byte bound",
            );
        }
        let mut inner = self.inner.lock().await;
        Self::prune(&mut inner);
        let Some(record) = inner.runs.get(run_id).cloned() else {
            return error(
                "human_shell_run_gone",
                "human shell run is unavailable or expired",
            );
        };
        let run = record.lock();
        if &run.scope != scope {
            return error(
                "human_shell_run_gone",
                "human shell run is unavailable or expired",
            );
        }
        let first = run
            .chunks
            .front()
            .map_or(run.next_sequence, |chunk| chunk.sequence);
        let resync_required = from_sequence < first;
        let mut remaining = max_bytes;
        let mut chunks = Vec::new();
        for chunk in run
            .chunks
            .iter()
            .filter(|chunk| chunk.sequence >= from_sequence.max(first))
        {
            if remaining == 0 {
                break;
            }
            let take = remaining.min(chunk.bytes.len());
            chunks.push(HumanShellOutputChunkDto {
                sequence: chunk.sequence,
                stderr: chunk.stderr,
                bytes: chunk.bytes[..take].to_vec(),
            });
            remaining -= take;
            if take < chunk.bytes.len() {
                break;
            }
        }
        let next_sequence = chunks.last().map_or(from_sequence.max(first), |chunk| {
            chunk.sequence.saturating_add(1)
        });
        CoreResponse::HumanShellRunOutput {
            run: run.meta.clone(),
            chunks,
            next_sequence,
            resync_required,
            output_truncated: run.output_truncated,
        }
    }

    pub async fn cancel(&self, scope: &HumanShellRunScope, run_id: &str) -> CoreResponse {
        let mut inner = self.inner.lock().await;
        Self::prune(&mut inner);
        let Some(record) = inner.runs.get(run_id).cloned() else {
            return error(
                "human_shell_run_gone",
                "human shell run is unavailable or expired",
            );
        };
        let run = record.lock();
        if &run.scope != scope {
            return error(
                "human_shell_run_gone",
                "human shell run is unavailable or expired",
            );
        }
        if let Some(handle) = &run.handle {
            handle.kill();
        }
        CoreResponse::HumanShellRunCancelled {
            run_id: run_id.to_owned(),
        }
    }

    fn prune(inner: &mut RegistryInner) {
        let now = chrono::Utc::now().timestamp_millis();
        let expired: Vec<String> = inner
            .runs
            .iter()
            .filter(|(_, run)| run.lock().meta.expires_at_ms <= now)
            .map(|(id, _)| id.clone())
            .collect();
        for id in expired {
            if let Some(run) = inner.runs.remove(&id) {
                let run = run.lock();
                if let Some(handle) = &run.handle {
                    handle.kill();
                }
                inner
                    .submissions
                    .remove(&(run.scope.principal_id.clone(), run.submission_id.clone()));
            }
        }
    }
}

fn append_output(run: &mut RunRecord, stderr: bool, bytes: &[u8]) {
    for part in bytes.chunks(HUMAN_SHELL_MAX_READ_BYTES) {
        let sequence = run.next_sequence;
        run.next_sequence = run.next_sequence.saturating_add(1);
        run.retained_bytes += part.len();
        run.chunks.push_back(HumanShellOutputChunkDto {
            sequence,
            stderr,
            bytes: part.to_vec(),
        });
        while run.retained_bytes > MAX_OUTPUT_BYTES || run.chunks.len() > MAX_RETAINED_CHUNKS {
            if let Some(old) = run.chunks.pop_front() {
                run.retained_bytes = run.retained_bytes.saturating_sub(old.bytes.len());
                run.output_truncated = true;
            } else {
                break;
            }
        }
    }
}

fn error(code: &str, message: &str) -> CoreResponse {
    CoreResponse::Error {
        code: code.to_owned(),
        message: message.to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use codegg_protocol::human_shell::HumanShellOriginDto;

    fn service() -> HumanShellRunService {
        let config = crate::scheduler::config::ResolvedSchedulerConfig::default();
        HumanShellRunService::new(Arc::new(
            crate::scheduler::admission::AdmissionController::new(config),
        ))
    }

    fn create_request(
        submission_id: &str,
        project: &str,
        workspace: &str,
        command: &str,
    ) -> HumanShellRunCreateRequestDto {
        HumanShellRunCreateRequestDto {
            protocol_version: HUMAN_SHELL_PROTOCOL_VERSION,
            submission_id: submission_id.into(),
            project_id: project.into(),
            workspace_id: workspace.into(),
            session_id: None,
            origin: HumanShellOriginDto::Bang,
            confirmed_dangerous: false,
            command: command.into(),
            timeout_secs: 300,
        }
    }

    async fn read_to_exit(
        service: &HumanShellRunService,
        scope: &HumanShellRunScope,
        run_id: &str,
    ) -> String {
        let mut cursor = 0;
        let mut output = Vec::new();
        for _ in 0..100 {
            let response = service
                .read(scope, run_id, cursor, HUMAN_SHELL_MAX_READ_BYTES)
                .await;
            if let CoreResponse::HumanShellRunOutput {
                run,
                chunks,
                next_sequence,
                ..
            } = response
            {
                for chunk in chunks {
                    output.extend(chunk.bytes);
                }
                cursor = next_sequence;
                if run.status != HumanShellRunStatusDto::Running {
                    break;
                }
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        String::from_utf8_lossy(&output).into_owned()
    }

    #[tokio::test]
    async fn two_principals_and_workspaces_keep_cwd_and_output_isolated() {
        let service = service();
        let a = tempfile::tempdir().unwrap();
        let b = tempfile::tempdir().unwrap();
        let scope_a = HumanShellRunScope {
            principal_id: "principal-a".into(),
            project_id: "project-a".into(),
            workspace_id: "workspace-a".into(),
            session_id: None,
        };
        let scope_b = HumanShellRunScope {
            principal_id: "principal-b".into(),
            project_id: "project-b".into(),
            workspace_id: "workspace-b".into(),
            session_id: None,
        };
        let response_a = service
            .create(
                scope_a.clone(),
                create_request(
                    "submission-a",
                    "project-a",
                    "workspace-a",
                    "pwd; printf marker-a",
                ),
                a.path().to_path_buf(),
                "daemon-a".into(),
                "sh".into(),
                None,
            )
            .await;
        let response_b = service
            .create(
                scope_b.clone(),
                create_request(
                    "submission-b",
                    "project-b",
                    "workspace-b",
                    "pwd; printf marker-b",
                ),
                b.path().to_path_buf(),
                "daemon-b".into(),
                "sh".into(),
                None,
            )
            .await;
        let CoreResponse::HumanShellRunCreated { run: run_a, .. } = response_a else {
            panic!("unexpected create response: {response_a:?}")
        };
        let CoreResponse::HumanShellRunCreated { run: run_b, .. } = response_b else {
            panic!("unexpected create response: {response_b:?}")
        };
        let output_a = read_to_exit(&service, &scope_a, &run_a.run_id).await;
        let output_b = read_to_exit(&service, &scope_b, &run_b.run_id).await;
        assert!(output_a.contains(a.path().to_str().unwrap()));
        assert!(output_a.contains("marker-a"));
        assert!(!output_a.contains("marker-b"));
        assert!(output_b.contains(b.path().to_str().unwrap()));
        assert!(output_b.contains("marker-b"));
        assert!(!output_b.contains("marker-a"));
        assert!(
            matches!(service.read(&scope_b, &run_a.run_id, 0, 1024).await, CoreResponse::Error { code, .. } if code == "human_shell_run_gone")
        );
        assert!(
            matches!(service.cancel(&scope_b, &run_a.run_id).await, CoreResponse::Error { code, .. } if code == "human_shell_run_gone")
        );
    }

    #[tokio::test]
    async fn configured_timeout_is_enforced_by_the_managed_process() {
        let service = service();
        let dir = tempfile::tempdir().unwrap();
        let scope = HumanShellRunScope {
            principal_id: "p".into(),
            project_id: "project".into(),
            workspace_id: "workspace".into(),
            session_id: None,
        };
        let mut oversized_timeout = create_request("too-long", "project", "workspace", "true");
        oversized_timeout.timeout_secs =
            codegg_protocol::human_shell::HUMAN_SHELL_MAX_TIMEOUT_SECS + 1;
        assert!(matches!(
            service
                .create(
                    scope.clone(),
                    oversized_timeout,
                    dir.path().into(),
                    "daemon".into(),
                    "sh".into(),
                    None,
                )
                .await,
            CoreResponse::Error { code, .. } if code == "human_shell_invalid_request"
        ));
        let mut request = create_request("timeout", "project", "workspace", "sleep 2");
        request.timeout_secs = 1;
        let response = service
            .create(
                scope.clone(),
                request,
                dir.path().into(),
                "daemon".into(),
                "sh".into(),
                None,
            )
            .await;
        let CoreResponse::HumanShellRunCreated { run, .. } = response else {
            panic!("unexpected create response: {response:?}")
        };
        for _ in 0..100 {
            if let CoreResponse::HumanShellRunOutput { run: status, .. } =
                service.read(&scope, &run.run_id, 0, 1024).await
            {
                if status.status == HumanShellRunStatusDto::TimedOut {
                    return;
                }
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        panic!("configured one-second timeout did not terminate the process");
    }

    #[tokio::test]
    async fn duplicate_admission_is_idempotent_and_changed_content_conflicts() {
        let service = service();
        let dir = tempfile::tempdir().unwrap();
        let scope = HumanShellRunScope {
            principal_id: "p".into(),
            project_id: "project".into(),
            workspace_id: "workspace".into(),
            session_id: None,
        };
        let request = create_request("same", "project", "workspace", "printf once");
        let first = service
            .create(
                scope.clone(),
                request.clone(),
                dir.path().into(),
                "daemon".into(),
                "sh".into(),
                None,
            )
            .await;
        let CoreResponse::HumanShellRunCreated {
            run,
            duplicate: false,
        } = first
        else {
            panic!("unexpected create response: {first:?}")
        };
        let retry = service
            .create(
                scope.clone(),
                request,
                dir.path().into(),
                "daemon".into(),
                "sh".into(),
                None,
            )
            .await;
        assert!(
            matches!(retry, CoreResponse::HumanShellRunCreated { duplicate: true, run: again } if again.run_id == run.run_id)
        );
        let changed = service
            .create(
                scope,
                create_request("same", "project", "workspace", "printf twice"),
                dir.path().into(),
                "daemon".into(),
                "sh".into(),
                None,
            )
            .await;
        assert!(
            matches!(changed, CoreResponse::Error { code, .. } if code == "human_shell_submission_conflict")
        );
    }

    #[tokio::test]
    async fn concurrent_process_slot_use_is_refused_before_a_second_spawn() {
        let mut config = crate::scheduler::config::ResolvedSchedulerConfig::default();
        config.resources.max_process_slots = 1;
        let service = HumanShellRunService::new(Arc::new(
            crate::scheduler::admission::AdmissionController::new(config),
        ));
        let dir = tempfile::tempdir().unwrap();
        let scope = HumanShellRunScope {
            principal_id: "p".into(),
            project_id: "project".into(),
            workspace_id: "workspace".into(),
            session_id: None,
        };
        let first = service
            .create(
                scope.clone(),
                create_request("first", "project", "workspace", "sleep 2"),
                dir.path().into(),
                "daemon".into(),
                "sh".into(),
                None,
            )
            .await;
        let CoreResponse::HumanShellRunCreated { run, .. } = first else {
            panic!("first run should be admitted: {first:?}");
        };
        let second = service
            .create(
                scope.clone(),
                create_request("second", "project", "workspace", "printf must-not-run"),
                dir.path().into(),
                "daemon".into(),
                "sh".into(),
                None,
            )
            .await;
        assert!(
            matches!(second, CoreResponse::Error { code, .. } if code == "human_shell_capacity")
        );
        assert!(matches!(
            service.cancel(&scope, &run.run_id).await,
            CoreResponse::HumanShellRunCancelled { run_id } if run_id == run.run_id
        ));
    }

    #[tokio::test]
    async fn cancellation_terminates_the_managed_process_and_restart_loses_ephemeral_handle() {
        let service = service();
        let dir = tempfile::tempdir().unwrap();
        let scope = HumanShellRunScope {
            principal_id: "p".into(),
            project_id: "project".into(),
            workspace_id: "workspace".into(),
            session_id: None,
        };
        let response = service
            .create(
                scope.clone(),
                create_request("cancel", "project", "workspace", "sleep 5; printf late"),
                dir.path().into(),
                "daemon".into(),
                "sh".into(),
                None,
            )
            .await;
        let CoreResponse::HumanShellRunCreated { run, .. } = response else {
            panic!("unexpected create response: {response:?}")
        };
        assert!(matches!(
            service.cancel(&scope, &run.run_id).await,
            CoreResponse::HumanShellRunCancelled { .. }
        ));
        let _ = read_to_exit(&service, &scope, &run.run_id).await;
        let restarted = HumanShellRunService::new(Arc::new(
            crate::scheduler::admission::AdmissionController::new(
                crate::scheduler::config::ResolvedSchedulerConfig::default(),
            ),
        ));
        assert!(
            matches!(restarted.read(&scope, &run.run_id, 0, 1024).await, CoreResponse::Error { code, .. } if code == "human_shell_run_gone")
        );
    }

    #[test]
    fn retained_output_has_a_hard_bound_and_advances_loss_cursor() {
        let scope = HumanShellRunScope {
            principal_id: "p".into(),
            project_id: "project".into(),
            workspace_id: "workspace".into(),
            session_id: None,
        };
        let mut run = RunRecord {
            meta: HumanShellRunMetadataDto {
                run_id: "run".into(),
                project_id: "project".into(),
                workspace_id: "workspace".into(),
                session_id: None,
                execution_node: "daemon".into(),
                cwd: "/tmp".into(),
                command: "x".into(),
                origin: HumanShellOriginDto::Bang,
                status: HumanShellRunStatusDto::Running,
                exit_code: None,
                expires_at_ms: 0,
            },
            scope,
            submission_id: "s".into(),
            command_digest: [0; 32],
            next_sequence: 1,
            chunks: VecDeque::new(),
            retained_bytes: 0,
            output_truncated: false,
            handle: None,
        };
        append_output(&mut run, false, &vec![b'x'; MAX_OUTPUT_BYTES + 100]);
        assert!(run.retained_bytes <= MAX_OUTPUT_BYTES);
        assert!(run.output_truncated);
        assert!(run.chunks.front().unwrap().sequence > 1);
    }
}

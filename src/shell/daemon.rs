//! Daemon-owned finite human shell runs.

use sha2::Digest;
use std::collections::{HashMap, VecDeque};
use std::panic::AssertUnwindSafe;
use std::sync::Arc;
use std::time::Duration;

use codegg_protocol::core::CoreResponse;
use codegg_protocol::human_shell::{
    HumanShellOriginDto, HumanShellOutputChunkDto, HumanShellRunCreateRequestDto,
    HumanShellRunMetadataDto, HumanShellRunStatusDto, HUMAN_SHELL_MAX_COMMAND_BYTES,
    HUMAN_SHELL_MAX_READ_BYTES, HUMAN_SHELL_PROTOCOL_VERSION,
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

/// Ephemeral run authority and bounded output retention; no command is replayed.
pub struct HumanShellRunService {
    inner: tokio::sync::Mutex<RegistryInner>,
    admission: Arc<crate::scheduler::admission::AdmissionController>,
    /// M005 corrective C005: the daemon's process admission authority. A
    /// human-originated shell command is a process launch, refused under
    /// the restricted shared profile; `None` means no durable authority.
    process_admission: Option<Arc<codegg_core::process_admission::ProcessAdmissionService>>,
    /// The daemon's lifecycle publisher. `None` means nothing listens.
    /// Every run is published, including a run with no session: the
    /// consumer keeps the runs it can attribute.
    publisher: Option<Arc<dyn Fn(HumanShellRunMetadataDto) + Send + Sync>>,
}

impl HumanShellRunService {
    pub fn new(admission: Arc<crate::scheduler::admission::AdmissionController>) -> Self {
        Self {
            inner: tokio::sync::Mutex::new(RegistryInner::default()),
            admission,
            process_admission: None,
            publisher: None,
        }
    }

    /// M005 corrective C005: install the daemon's process admission
    /// authority.
    pub fn set_process_admission(
        &mut self,
        service: Arc<codegg_core::process_admission::ProcessAdmissionService>,
    ) {
        self.process_admission = Some(service);
    }

    /// Install the publisher that announces this service's run lifecycle.
    pub fn with_publisher(
        mut self,
        publisher: Arc<dyn Fn(HumanShellRunMetadataDto) + Send + Sync>,
    ) -> Self {
        self.publisher = Some(publisher);
        self
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
        // M005 corrective C005: the restricted shared-checkout profile
        // refuses a human-originated shell command before the process
        // exists, exactly as it refuses a model tool's shell.
        // c005-denial: human-shell-gate
        if let Some(service) = self.process_admission.as_ref() {
            let admission = service
                .admit_scope(&cwd, codegg_core::process_admission::ProcessKind::Command)
                .await;
            let admission = match admission {
                Ok(admission) => admission,
                Err(failure) => {
                    return error(
                        "human_shell_process_check_failed",
                        &format!(
                            "the process admission authority could not be consulted: {failure}"
                        ),
                    )
                }
            };
            if let Some(denial) = admission.denial() {
                return error("human_shell_process_denied", &denial.to_string());
            }
        }
        let mut runtime = super::runtime::ShellRuntime::with_host_shell(shell);
        if let Some(spec) = sandbox {
            runtime = runtime.with_sandbox(spec);
        }
        // M005 corrective C005: the runtime carries the gate's policy too.
        if let Some(service) = self.process_admission.as_ref() {
            if let Some(policy) = crate::tool::process_admission::build_process_admission(
                Some(service.clone()),
                &cwd,
                None,
            ) {
                runtime = runtime.with_process_admission(policy);
            }
        }
        let handle = match runtime.spawn(shell_request, event_tx).await {
            Ok(handle) => handle,
            Err(message) => return error("human_shell_start_failed", &message),
        };
        // M005 corrective C005: a persistent shell started under an older
        // authority must block a later shared reservation of its checkout.
        let process_id = format!("human_shell:{run_id}");
        if let Some(service) = self.process_admission.as_ref() {
            service
                .processes()
                .register(codegg_core::process_admission::PersistentProcessEntry {
                    process_id: process_id.clone(),
                    kind: codegg_core::process_admission::ProcessKind::Command,
                    checkout_root: cwd.clone(),
                });
        }
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
            started_at_ms: Some(now),
            completed_at_ms: None,
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
        // M005 corrective C005: the registry entry is released when the run ends.
        let release_registry = self
            .process_admission
            .as_ref()
            .map(|service| (service.clone(), process_id.clone()));
        let publisher = self.publisher.clone();
        let task_publisher = publisher.clone();
        tokio::spawn(async move {
            let _permit = permit;
            while let Some(event) = event_rx.recv().await {
                let mut run = record.lock();
                let before = run.meta.status.clone();
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
                        run.meta.completed_at_ms = Some(chrono::Utc::now().timestamp_millis());
                        run.handle = None;
                    }
                    crate::shell::ShellEvent::TimedOut { .. } => {
                        run.meta.status = HumanShellRunStatusDto::TimedOut;
                        run.meta.completed_at_ms = Some(chrono::Utc::now().timestamp_millis());
                        run.handle = None;
                    }
                    crate::shell::ShellEvent::FailedToStart { .. } => {
                        run.meta.status = HumanShellRunStatusDto::Failed;
                        run.meta.completed_at_ms = Some(chrono::Utc::now().timestamp_millis());
                        run.handle = None;
                    }
                    crate::shell::ShellEvent::Started { .. } => {}
                }
                let snapshot = (run.meta.status != before).then(|| run.meta.clone());
                drop(run);
                if let Some(snapshot) = snapshot {
                    publish_run(&task_publisher, &snapshot);
                }
            }
            let mut run = record.lock();
            let snapshot = if run.meta.status == HumanShellRunStatusDto::Running {
                run.meta.status = HumanShellRunStatusDto::Lost;
                run.meta.completed_at_ms = Some(chrono::Utc::now().timestamp_millis());
                run.handle = None;
                Some(run.meta.clone())
            } else {
                None
            };
            drop(run);
            if let Some(snapshot) = snapshot {
                publish_run(&task_publisher, &snapshot);
            }
            if let Some((service, process_id)) = release_registry {
                service.processes().release(&process_id);
            }
        });
        drop(inner);
        publish_run(&publisher, &meta);
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

    /// The human-shell evidence for one run, assembled from the daemon's
    /// **own** run bytes.
    ///
    /// M007-I (work package E): the frontend no longer assembles the
    /// evidence from its own mirror of the run. The daemon reads its own
    /// bytes, renders the digest and returns the assembled text; the
    /// promotion staging then redacts it. The bytes and the redaction are
    /// therefore one daemon-owned path.
    pub async fn evidence_text(&self, run_id: &str, question: &str) -> Result<String, String> {
        let Some(scope) = self.scope_for(run_id).await else {
            return Err("the shell run is unavailable or expired".to_owned());
        };
        let read = self
            .read(&scope, run_id, 0, HUMAN_SHELL_MAX_READ_BYTES)
            .await;
        let CoreResponse::HumanShellRunOutput {
            run,
            chunks,
            output_truncated,
            ..
        } = read
        else {
            return Err("the shell run is unavailable or expired".to_owned());
        };
        let mut stdout = crate::shell::store::BoundedOutput::new();
        let mut stderr = crate::shell::store::BoundedOutput::new();
        for chunk in &chunks {
            if chunk.stderr {
                stderr.append(&chunk.bytes);
            } else {
                stdout.append(&chunk.bytes);
            }
        }
        let status = match run.status {
            crate::protocol::human_shell::HumanShellRunStatusDto::Running => {
                crate::shell::types::ShellStatus::Running
            }
            crate::protocol::human_shell::HumanShellRunStatusDto::Exited => {
                crate::shell::types::ShellStatus::Exited
            }
            crate::protocol::human_shell::HumanShellRunStatusDto::TimedOut => {
                crate::shell::types::ShellStatus::TimedOut
            }
            crate::protocol::human_shell::HumanShellRunStatusDto::Cancelled => {
                crate::shell::types::ShellStatus::Killed
            }
            crate::protocol::human_shell::HumanShellRunStatusDto::Failed => {
                crate::shell::types::ShellStatus::FailedToStart
            }
            crate::protocol::human_shell::HumanShellRunStatusDto::Lost => {
                crate::shell::types::ShellStatus::FailedToStart
            }
        };
        if status != crate::shell::types::ShellStatus::Exited {
            return Err("shell output can only be promoted after the command exits".to_owned());
        }
        let elapsed = match (run.started_at_ms, run.completed_at_ms) {
            (Some(started), Some(completed)) => {
                std::time::Duration::from_millis(completed.saturating_sub(started).max(0) as u64)
            }
            _ => std::time::Duration::default(),
        };
        let digest = crate::shell::ShellDigest::build(
            &run.command,
            std::path::Path::new(&run.cwd),
            status,
            run.exit_code,
            elapsed,
            &stdout,
            &stderr,
        );
        let mut text = format!(
            "Using the attached shell output, answer: {question}\n\n{}",
            digest.render()
        );
        if output_truncated {
            text.push_str("\n[shell output truncated]");
        }
        Ok(text)
    }

    /// The session's runs, newest first; the listing carries no output.
    pub async fn list_for_session(&self, session_id: &str) -> Vec<HumanShellRunMetadataDto> {
        let mut inner = self.inner.lock().await;
        Self::prune(&mut inner);
        let mut runs: Vec<HumanShellRunMetadataDto> = inner
            .runs
            .values()
            .filter_map(|record| {
                let run = record.lock();
                (run.scope.session_id.as_deref() == Some(session_id)).then(|| run.meta.clone())
            })
            .collect();
        runs.sort_by(|left, right| {
            run_order_key(right)
                .cmp(&run_order_key(left))
                .then_with(|| left.run_id.cmp(&right.run_id))
        });
        runs
    }

    /// The runs in one exact scope, newest first and bounded by `limit`.
    ///
    /// The scope is exact — principal, project, workspace and session.
    pub async fn list_for_scope(
        &self,
        scope: &HumanShellRunScope,
        limit: usize,
    ) -> Vec<HumanShellRunMetadataDto> {
        let mut inner = self.inner.lock().await;
        Self::prune(&mut inner);
        let mut runs: Vec<HumanShellRunMetadataDto> = inner
            .runs
            .values()
            .filter_map(|record| {
                let run = record.lock();
                (run.scope.principal_id == scope.principal_id
                    && run.scope.project_id == scope.project_id
                    && run.scope.workspace_id == scope.workspace_id
                    && run.scope.session_id == scope.session_id)
                    .then(|| run.meta.clone())
            })
            .collect();
        runs.sort_by(|left, right| {
            run_order_key(right)
                .cmp(&run_order_key(left))
                .then_with(|| left.run_id.cmp(&right.run_id))
        });
        runs.truncate(limit);
        runs
    }

    /// The run's own binding: a cancel's authority, for a run this daemon
    /// still holds. The runs live in memory, so an absent run answers `None`.
    pub async fn scope_for(&self, run_id: &str) -> Option<HumanShellRunScope> {
        let mut inner = self.inner.lock().await;
        Self::prune(&mut inner);
        let record = inner.runs.get(run_id).cloned()?;
        let run = record.lock();
        Some(run.scope.clone())
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

/// The projection's bounded view of one run.
pub fn run_projection(
    meta: &HumanShellRunMetadataDto,
) -> codegg_protocol::projection::dto::ShellRunProjection {
    codegg_protocol::projection::dto::ShellRunProjection {
        run_id: meta.run_id.clone(),
        command: meta.command.clone(),
        cwd: meta.cwd.clone(),
        origin: origin_name(meta.origin).to_owned(),
        status: status_name(&meta.status).to_owned(),
        started_at_ms: meta.started_at_ms,
        completed_at_ms: meta.completed_at_ms,
        exit_code: meta.exit_code,
    }
}

/// Announce one run's metadata. A panicking publisher is contained, so a
/// run's lifecycle never depends on it.
fn publish_run(
    publisher: &Option<Arc<dyn Fn(HumanShellRunMetadataDto) + Send + Sync>>,
    meta: &HumanShellRunMetadataDto,
) {
    let Some(publisher) = publisher.as_ref() else {
        return;
    };
    if std::panic::catch_unwind(AssertUnwindSafe(|| publisher(meta.clone()))).is_err() {
        tracing::warn!(run_id = %meta.run_id, "the human shell run publisher panicked");
    }
}

fn run_order_key(meta: &HumanShellRunMetadataDto) -> (i64, i64) {
    (meta.started_at_ms.unwrap_or(i64::MIN), meta.expires_at_ms)
}

fn status_name(status: &HumanShellRunStatusDto) -> &'static str {
    match status {
        HumanShellRunStatusDto::Running => "running",
        HumanShellRunStatusDto::Exited => "exited",
        HumanShellRunStatusDto::TimedOut => "timed_out",
        HumanShellRunStatusDto::Cancelled => "cancelled",
        HumanShellRunStatusDto::Failed => "failed",
        HumanShellRunStatusDto::Lost => "lost",
    }
}

fn origin_name(origin: HumanShellOriginDto) -> &'static str {
    match origin {
        HumanShellOriginDto::Bang => "bang",
        HumanShellOriginDto::BangBang => "bang_bang",
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

    fn service_with_publisher(
        publisher: Arc<dyn Fn(HumanShellRunMetadataDto) + Send + Sync>,
    ) -> HumanShellRunService {
        let config = crate::scheduler::config::ResolvedSchedulerConfig::default();
        HumanShellRunService::new(Arc::new(
            crate::scheduler::admission::AdmissionController::new(config),
        ))
        .with_publisher(publisher)
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

    fn scoped_request(
        submission_id: &str,
        scope: &HumanShellRunScope,
        command: &str,
    ) -> HumanShellRunCreateRequestDto {
        let mut request = create_request(
            submission_id,
            &scope.project_id,
            &scope.workspace_id,
            command,
        );
        request.session_id = scope.session_id.clone();
        request
    }

    async fn create_run(
        service: &HumanShellRunService,
        scope: &HumanShellRunScope,
        cwd: &std::path::Path,
        submission_id: &str,
        command: &str,
    ) -> HumanShellRunMetadataDto {
        let response = service
            .create(
                scope.clone(),
                scoped_request(submission_id, scope, command),
                cwd.to_path_buf(),
                "daemon".into(),
                "sh".into(),
                None,
            )
            .await;
        match response {
            CoreResponse::HumanShellRunCreated { run, .. } => run,
            other => panic!("unexpected create response: {other:?}"),
        }
    }

    async fn wait_for_terminal(
        service: &HumanShellRunService,
        scope: &HumanShellRunScope,
        run_id: &str,
    ) -> HumanShellRunStatusDto {
        for _ in 0..200 {
            if let CoreResponse::HumanShellRunOutput { run, .. } = service
                .read(scope, run_id, 0, HUMAN_SHELL_MAX_READ_BYTES)
                .await
            {
                if run.status != HumanShellRunStatusDto::Running {
                    return run.status;
                }
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        panic!("the run never reached a terminal status");
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
                started_at_ms: None,
                completed_at_ms: None,
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

    #[tokio::test]
    async fn list_for_session_returns_the_session_runs_newest_first_without_output() {
        let service = service();
        let dir = tempfile::tempdir().unwrap();
        let scope = HumanShellRunScope {
            principal_id: "p".into(),
            project_id: "project".into(),
            workspace_id: "workspace".into(),
            session_id: Some("session-a".into()),
        };
        let other = HumanShellRunScope {
            session_id: Some("session-b".into()),
            ..scope.clone()
        };
        let first = create_run(
            &service,
            &scope,
            dir.path(),
            "first",
            "printf z; printf z; printf 9",
        )
        .await;
        tokio::time::sleep(Duration::from_millis(20)).await;
        let second = create_run(&service, &scope, dir.path(), "second", "printf y").await;
        tokio::time::sleep(Duration::from_millis(20)).await;
        let third = create_run(&service, &other, dir.path(), "third", "printf x").await;

        let listed = service.list_for_session("session-a").await;
        assert_eq!(listed.len(), 2);
        assert_eq!(listed[0].run_id, second.run_id);
        assert_eq!(listed[1].run_id, first.run_id);
        assert!(listed[0].started_at_ms > listed[1].started_at_ms);
        assert!(listed
            .iter()
            .all(|run| run.session_id.as_deref() == Some("session-a")));

        let other_listed = service.list_for_session("session-b").await;
        assert_eq!(other_listed.len(), 1);
        assert_eq!(other_listed[0].run_id, third.run_id);
        assert!(service.list_for_session("session-c").await.is_empty());

        let output = read_to_exit(&service, &scope, &first.run_id).await;
        assert!(output.contains("zz9"));
        let serialised = serde_json::to_string(&listed[1]).unwrap();
        assert!(!serialised.contains("zz9"));
        assert!(!serialised.contains("bytes"));
        assert!(!serialised.contains("chunks"));
    }

    #[tokio::test]
    async fn the_publisher_observes_the_creation_and_the_terminal_status() {
        let observed: Arc<Mutex<Vec<HumanShellRunMetadataDto>>> = Arc::new(Mutex::new(Vec::new()));
        let sink = observed.clone();
        let service = service_with_publisher(Arc::new(move |meta: HumanShellRunMetadataDto| {
            sink.lock().push(meta);
        }));
        let dir = tempfile::tempdir().unwrap();
        let scope = HumanShellRunScope {
            principal_id: "p".into(),
            project_id: "project".into(),
            workspace_id: "workspace".into(),
            session_id: Some("session-a".into()),
        };
        let run = create_run(&service, &scope, dir.path(), "publisher", "printf done").await;

        let mut terminal = None;
        for _ in 0..200 {
            {
                let seen = observed.lock();
                if let Some(status) = seen
                    .iter()
                    .find(|meta| {
                        meta.run_id == run.run_id && meta.status != HumanShellRunStatusDto::Running
                    })
                    .map(|meta| meta.status.clone())
                {
                    terminal = Some(status);
                }
            }
            if terminal.is_some() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        assert_eq!(terminal, Some(HumanShellRunStatusDto::Exited));

        let seen = observed.lock();
        assert!(seen.iter().any(|meta| {
            meta.run_id == run.run_id && meta.status == HumanShellRunStatusDto::Running
        }));
        assert!(seen.iter().any(|meta| {
            meta.run_id == run.run_id && meta.status == HumanShellRunStatusDto::Exited
        }));
    }

    #[tokio::test]
    async fn a_panicking_publisher_cannot_fail_a_run() {
        let service = service_with_publisher(Arc::new(|_meta: HumanShellRunMetadataDto| {
            panic!("this publisher is broken");
        }));
        let dir = tempfile::tempdir().unwrap();
        let scope = HumanShellRunScope {
            principal_id: "p".into(),
            project_id: "project".into(),
            workspace_id: "workspace".into(),
            session_id: Some("session-a".into()),
        };
        let run = create_run(&service, &scope, dir.path(), "broken", "printf ok").await;
        assert_eq!(
            wait_for_terminal(&service, &scope, &run.run_id).await,
            HumanShellRunStatusDto::Exited
        );
        let listed = service.list_for_session("session-a").await;
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].status, HumanShellRunStatusDto::Exited);
        assert_eq!(listed[0].exit_code, Some(0));
    }

    #[test]
    fn projection_names_match_the_serde_names() {
        let statuses = [
            (HumanShellRunStatusDto::Running, "running"),
            (HumanShellRunStatusDto::Exited, "exited"),
            (HumanShellRunStatusDto::TimedOut, "timed_out"),
            (HumanShellRunStatusDto::Cancelled, "cancelled"),
            (HumanShellRunStatusDto::Failed, "failed"),
            (HumanShellRunStatusDto::Lost, "lost"),
        ];
        for (status, name) in statuses {
            assert_eq!(
                serde_json::to_value(&status).unwrap(),
                serde_json::json!(name)
            );
            assert_eq!(status_name(&status), name);
        }
        let origins = [
            (HumanShellOriginDto::Bang, "bang"),
            (HumanShellOriginDto::BangBang, "bang_bang"),
        ];
        for (origin, name) in origins {
            assert_eq!(
                serde_json::to_value(origin).unwrap(),
                serde_json::json!(name)
            );
            assert_eq!(origin_name(origin), name);
        }
    }
}

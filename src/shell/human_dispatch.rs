//! Daemon-owned finite human-shell run handles.
//!
//! The TUI is a request/render adapter. This registry owns the bounded
//! output cursor and cancellation handle while `ShellRuntime` and
//! `ManagedProcessService` own actual process lifecycle.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use dashmap::DashMap;
use parking_lot::Mutex;
use tokio::sync::mpsc;

use crate::protocol::core::CoreResponse;
use crate::protocol::human_shell::{
    HumanShellRunCreateDto, HumanShellRunOutputDto, HumanShellRunSnapshotDto,
    HumanShellRunStatusDto, HumanShellRunStreamDto, HUMAN_SHELL_RUN_MAX_ACTIVE,
    HUMAN_SHELL_RUN_MAX_OUTPUT_BYTES, HUMAN_SHELL_RUN_MAX_READ_BYTES, HUMAN_SHELL_RUN_MAX_RETAINED,
};
use crate::scheduler::admission::{AdmissionController, AdmissionDecision};
use crate::scheduler::permit::{PermitDimensions, ResourcePermitGuard};
use crate::shell::types::{
    ShellCapturePolicy, ShellCommandId, ShellEnvPolicy, ShellEvent, ShellOrigin, ShellRequest,
};

const RUN_TTL: Duration = Duration::from_secs(10 * 60);
const RUN_TIMEOUT_MAX: Duration = Duration::from_secs(300);
const MAX_RUNS_PER_PRINCIPAL: usize = 16;
static NEXT_SHELL_COMMAND_ID: AtomicU64 = AtomicU64::new(1);

struct RunState {
    status: HumanShellRunStatusDto,
    exit_code: Option<i32>,
    cancelled: bool,
    output_bytes: usize,
    truncated: bool,
    next_cursor: u64,
    outputs: VecDeque<HumanShellRunOutputDto>,
}

struct RunRecord {
    id: String,
    owner: String,
    idempotency_key: String,
    command_digest: String,
    cwd_relative: Option<String>,
    timeout_secs: u64,
    session_id: Option<String>,
    workspace_id: String,
    created_at: Instant,
    handle: Mutex<Option<crate::shell::ShellHandle>>,
    state: Mutex<RunState>,
}

impl RunRecord {
    fn snapshot(&self) -> HumanShellRunSnapshotDto {
        let state = self.state.lock();
        HumanShellRunSnapshotDto {
            run_id: self.id.clone(),
            workspace_id: self.workspace_id.clone(),
            status: state.status,
            exit_code: state.exit_code,
            next_cursor: state.next_cursor,
            oldest_cursor: state
                .outputs
                .front()
                .map_or(state.next_cursor, |output| output.sequence),
            truncated: state.truncated,
        }
    }

    fn observe(&self, event: ShellEvent) {
        let mut state = self.state.lock();
        match event {
            ShellEvent::Stdout { bytes, .. } => {
                push_output(&mut state, HumanShellRunStreamDto::Stdout, bytes)
            }
            ShellEvent::Stderr { bytes, .. } => {
                push_output(&mut state, HumanShellRunStreamDto::Stderr, bytes)
            }
            ShellEvent::OutputTruncated { .. } => {}
            ShellEvent::Exited { status, .. } => {
                state.status = if state.cancelled {
                    HumanShellRunStatusDto::Cancelled
                } else {
                    HumanShellRunStatusDto::Exited
                };
                state.exit_code = status;
            }
            ShellEvent::TimedOut { .. } => {
                state.status = if state.cancelled {
                    HumanShellRunStatusDto::Cancelled
                } else {
                    HumanShellRunStatusDto::TimedOut
                }
            }
            ShellEvent::FailedToStart { .. } => {
                state.status = if state.cancelled {
                    HumanShellRunStatusDto::Cancelled
                } else {
                    HumanShellRunStatusDto::Failed
                }
            }
            ShellEvent::Started { .. } => {}
        }
    }
}

fn push_output(state: &mut RunState, stream: HumanShellRunStreamDto, bytes: Vec<u8>) {
    if bytes.is_empty() {
        return;
    }
    let mut offset = 0;
    while offset < bytes.len() {
        let remaining = HUMAN_SHELL_RUN_MAX_OUTPUT_BYTES.saturating_sub(state.output_bytes);
        if remaining == 0 {
            state.truncated = true;
            return;
        }
        let take = remaining.min(bytes.len() - offset);
        state.outputs.push_back(HumanShellRunOutputDto {
            sequence: state.next_cursor,
            stream,
            bytes: bytes[offset..offset + take].to_vec(),
        });
        state.next_cursor = state.next_cursor.saturating_add(1);
        state.output_bytes += take;
        offset += take;
    }
    if offset < bytes.len() {
        state.truncated = true;
    }
}

/// Bounded daemon-process-local run registry.
pub struct HumanShellRunProtocol {
    admission: Arc<AdmissionController>,
    create_gate: Mutex<()>,
    runs: DashMap<String, Arc<RunRecord>>,
    idempotency: DashMap<String, Arc<RunRecord>>,
}

impl Drop for HumanShellRunProtocol {
    fn drop(&mut self) {
        for run in self.runs.iter() {
            if let Some(handle) = run.handle.lock().as_ref() {
                handle.kill();
            }
        }
    }
}

impl HumanShellRunProtocol {
    pub fn new(admission: Arc<AdmissionController>) -> Self {
        Self {
            admission,
            create_gate: Mutex::new(()),
            runs: DashMap::new(),
            idempotency: DashMap::new(),
        }
    }

    pub async fn create(
        &self,
        owner: &str,
        request: &HumanShellRunCreateDto,
        ctx: Arc<codegg_core::workspace::ExecutionContext>,
    ) -> CoreResponse {
        if !request.validate() || request.workspace_id != ctx.workspace_id.as_str() {
            return run_error("human_shell_invalid_request", "invalid shell run request");
        }
        self.prune_expired();
        let session = request.session_id.as_deref().unwrap_or("local");
        let idempotency_key = format!(
            "{}:{owner}:{}:{session}:{}:{}",
            owner.len(),
            session.len(),
            request.idempotency_key.len(),
            request.idempotency_key
        );
        let command_digest =
            codegg_core::audit_instrumentation::structural_digest(request.command.as_bytes());
        if let Some(existing) = self.idempotency.get(&idempotency_key) {
            return duplicate_response(&existing, owner, request, &command_digest);
        }
        let cwd_relative = request.cwd_relative.as_deref().map(std::path::Path::new);
        let cwd = match ctx.resolve_relative_cwd(cwd_relative).await {
            Ok(path) => path,
            Err(_) => {
                return run_error(
                    "human_shell_invalid_cwd",
                    "shell working directory is unavailable",
                )
            }
        };
        let permit = match self
            .admission
            .try_admit_arc(&finite_human_shell_dimensions())
        {
            AdmissionDecision::Admitted(permit) => permit,
            AdmissionDecision::TemporarilyBlocked(_) => {
                return run_error(
                    "human_shell_admission_blocked",
                    "scheduler capacity is temporarily unavailable",
                )
            }
            AdmissionDecision::Impossible(_) => {
                return run_error(
                    "human_shell_admission_impossible",
                    "scheduler cannot admit this process",
                )
            }
        };

        let run_id = uuid::Uuid::new_v4().to_string();
        let local_id = ShellCommandId(NEXT_SHELL_COMMAND_ID.fetch_add(1, Ordering::Relaxed));
        let record = Arc::new(RunRecord {
            id: run_id.clone(),
            owner: owner.to_owned(),
            idempotency_key: idempotency_key.clone(),
            command_digest: command_digest.clone(),
            cwd_relative: request.cwd_relative.clone(),
            timeout_secs: request.timeout_secs,
            session_id: request.session_id.clone(),
            workspace_id: request.workspace_id.clone(),
            created_at: Instant::now(),
            handle: Mutex::new(None),
            state: Mutex::new(RunState {
                status: HumanShellRunStatusDto::Starting,
                exit_code: None,
                cancelled: false,
                output_bytes: 0,
                truncated: false,
                next_cursor: 0,
                outputs: VecDeque::new(),
            }),
        });
        let _create_guard = self.create_gate.lock();
        if let Some(existing) = self.idempotency.get(&idempotency_key) {
            drop(permit);
            return duplicate_response(&existing, owner, request, &command_digest);
        }
        let owner_runs = self.runs.iter().filter(|run| run.owner == owner).count();
        let active_runs = self
            .runs
            .iter()
            .filter(|run| {
                run.owner == owner
                    && matches!(
                        run.state.lock().status,
                        HumanShellRunStatusDto::Starting | HumanShellRunStatusDto::Running
                    )
            })
            .count();
        if active_runs >= HUMAN_SHELL_RUN_MAX_ACTIVE || owner_runs >= MAX_RUNS_PER_PRINCIPAL {
            drop(permit);
            return run_error("human_shell_run_limit", "human shell run limit reached");
        }
        if self.runs.len() >= HUMAN_SHELL_RUN_MAX_RETAINED {
            drop(permit);
            return run_error("human_shell_run_limit", "daemon shell run capacity is full");
        }
        match self.idempotency.entry(idempotency_key.clone()) {
            dashmap::mapref::entry::Entry::Occupied(entry) => {
                drop(permit);
                return duplicate_response(entry.get(), owner, request, &command_digest);
            }
            dashmap::mapref::entry::Entry::Vacant(entry) => {
                entry.insert(record.clone());
                self.runs.insert(run_id.clone(), record.clone());
            }
        }
        drop(_create_guard);
        let shell_request = ShellRequest {
            id: local_id,
            origin: ShellOrigin::HumanEphemeral,
            command: request.command.clone(),
            cwd,
            timeout: Duration::from_secs(request.timeout_secs).min(RUN_TIMEOUT_MAX),
            capture_policy: ShellCapturePolicy::StoreEphemeral,
            env_policy: ShellEnvPolicy::InheritHumanOwner,
        };
        let (tx, mut rx) = mpsc::channel(128);
        let handle = match crate::shell::ShellRuntime::new()
            .spawn(shell_request, tx)
            .await
        {
            Ok(handle) => handle,
            Err(_) => {
                self.runs.remove(&run_id);
                self.idempotency.remove(&idempotency_key);
                return run_error(
                    "human_shell_spawn_failed",
                    "shell process could not be started",
                );
            }
        };
        *record.handle.lock() = Some(handle);
        let cancelled_while_starting = {
            let mut state = record.state.lock();
            if state.cancelled {
                true
            } else {
                state.status = HumanShellRunStatusDto::Running;
                false
            }
        };
        if cancelled_while_starting {
            if let Some(handle) = record.handle.lock().as_ref() {
                handle.kill();
            }
        }
        let event_record = record.clone();
        tokio::spawn(async move {
            let _permit: ResourcePermitGuard = permit;
            while let Some(event) = rx.recv().await {
                event_record.observe(event);
                if !matches!(
                    event_record.state.lock().status,
                    HumanShellRunStatusDto::Starting | HumanShellRunStatusDto::Running
                ) {
                    event_record.handle.lock().take();
                }
            }
            let mut state = event_record.state.lock();
            if matches!(
                state.status,
                HumanShellRunStatusDto::Starting | HumanShellRunStatusDto::Running
            ) {
                state.status = if state.cancelled {
                    HumanShellRunStatusDto::Cancelled
                } else {
                    HumanShellRunStatusDto::Failed
                };
            }
            drop(state);
            event_record.handle.lock().take();
        });
        CoreResponse::HumanShellRunCreated {
            snapshot: record.snapshot(),
            duplicate: false,
        }
    }

    pub fn read(
        &self,
        owner: &str,
        session_id: Option<&str>,
        run_id: &str,
        from_cursor: u64,
        max_bytes: Option<usize>,
    ) -> CoreResponse {
        let Some(record) = self.owned(owner, session_id, run_id) else {
            return run_error("human_shell_run_gone", "shell run is unavailable");
        };
        let max_bytes = max_bytes.unwrap_or(HUMAN_SHELL_RUN_MAX_READ_BYTES);
        if max_bytes == 0 || max_bytes > HUMAN_SHELL_RUN_MAX_READ_BYTES {
            return run_error(
                "human_shell_invalid_request",
                "shell output read exceeds its bound",
            );
        }
        let state = record.state.lock();
        let oldest_cursor = state
            .outputs
            .front()
            .map_or(state.next_cursor, |chunk| chunk.sequence);
        let resync_required = from_cursor < oldest_cursor || from_cursor > state.next_cursor;
        let cursor = if from_cursor > state.next_cursor {
            oldest_cursor
        } else {
            from_cursor.max(oldest_cursor)
        };
        let mut output_bytes = 0;
        let mut outputs = Vec::new();
        for output in state
            .outputs
            .iter()
            .filter(|output| output.sequence >= cursor)
        {
            if output_bytes + output.bytes.len() > max_bytes {
                break;
            }
            output_bytes += output.bytes.len();
            outputs.push(output.clone());
        }
        let snapshot = HumanShellRunSnapshotDto {
            run_id: record.id.clone(),
            workspace_id: record.workspace_id.clone(),
            status: state.status,
            exit_code: state.exit_code,
            next_cursor: state.next_cursor,
            oldest_cursor,
            truncated: state.truncated || resync_required,
        };
        CoreResponse::HumanShellRunRead {
            snapshot,
            outputs,
            resync_required,
        }
    }

    pub fn cancel(&self, owner: &str, session_id: Option<&str>, run_id: &str) -> CoreResponse {
        let Some(record) = self.owned(owner, session_id, run_id) else {
            return run_error("human_shell_run_gone", "shell run is unavailable");
        };
        let handle = record.handle.lock().clone();
        record.state.lock().cancelled = true;
        if let Some(handle) = handle {
            handle.kill();
        }
        CoreResponse::HumanShellRunCancelled {
            snapshot: record.snapshot(),
        }
    }

    fn owned(&self, owner: &str, session_id: Option<&str>, run_id: &str) -> Option<Arc<RunRecord>> {
        let record = self.runs.get(run_id)?.clone();
        if record.owner != owner || record.session_id.as_deref() != session_id {
            return None;
        }
        Some(record)
    }

    fn prune_expired(&self) {
        let expired: Vec<String> = self
            .runs
            .iter()
            .filter(|run| {
                run.created_at.elapsed() >= RUN_TTL
                    && run.state.lock().status != HumanShellRunStatusDto::Running
            })
            .map(|run| run.id.clone())
            .collect();
        for run_id in expired {
            if let Some((_, record)) = self.runs.remove(&run_id) {
                self.idempotency.remove(&record.idempotency_key);
            }
        }
    }
}

fn duplicate_response(
    existing: &RunRecord,
    owner: &str,
    request: &HumanShellRunCreateDto,
    command_digest: &str,
) -> CoreResponse {
    if existing.owner != owner
        || existing.workspace_id != request.workspace_id
        || existing.session_id != request.session_id
        || existing.command_digest != command_digest
        || existing.cwd_relative != request.cwd_relative
        || existing.timeout_secs != request.timeout_secs
    {
        return run_error(
            "human_shell_idempotency_conflict",
            "shell run idempotency key was reused",
        );
    }
    CoreResponse::HumanShellRunCreated {
        snapshot: existing.snapshot(),
        duplicate: true,
    }
}

fn finite_human_shell_dimensions() -> PermitDimensions {
    crate::interactive_process::permit_dimensions_for_interactive()
}

fn run_error(code: &str, message: &str) -> CoreResponse {
    CoreResponse::Error {
        code: code.to_owned(),
        message: message.to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::human_shell::{HumanShellRunCreateDto, HUMAN_SHELL_RUN_PROTOCOL_VERSION};

    fn service() -> HumanShellRunProtocol {
        let config = crate::scheduler::config::ResolvedSchedulerConfig::default();
        HumanShellRunProtocol::new(Arc::new(AdmissionController::new(config)))
    }

    fn context(
        root: &std::path::Path,
        workspace_id: &str,
        session_id: &str,
    ) -> Arc<codegg_core::workspace::ExecutionContext> {
        let now = chrono::Utc::now();
        let record = Arc::new(codegg_core::workspace::WorkspaceRecord {
            id: codegg_core::workspace::WorkspaceId::new_unchecked(workspace_id.to_owned()),
            canonical_root: root.to_path_buf(),
            display_name: workspace_id.to_owned(),
            created_at: now,
            last_opened_at: now,
            archived_at: None,
        });
        codegg_core::workspace::ExecutionContext::new(
            record,
            Some(session_id.to_owned()),
            tokio_util::sync::CancellationToken::new(),
        )
    }

    fn request(workspace_id: &str, session_id: &str, command: &str) -> HumanShellRunCreateDto {
        HumanShellRunCreateDto {
            protocol_version: HUMAN_SHELL_RUN_PROTOCOL_VERSION,
            idempotency_key: uuid::Uuid::new_v4().to_string(),
            workspace_id: workspace_id.to_owned(),
            session_id: Some(session_id.to_owned()),
            cwd_relative: None,
            command: command.to_owned(),
            timeout_secs: 10,
        }
    }

    #[test]
    fn retained_output_is_bounded_and_marks_truncation() {
        let mut state = RunState {
            status: HumanShellRunStatusDto::Running,
            exit_code: None,
            cancelled: false,
            output_bytes: 0,
            truncated: false,
            next_cursor: 0,
            outputs: VecDeque::new(),
        };
        let input = vec![b'x'; HUMAN_SHELL_RUN_MAX_OUTPUT_BYTES + 1];
        push_output(&mut state, HumanShellRunStreamDto::Stdout, input);
        assert_eq!(state.output_bytes, HUMAN_SHELL_RUN_MAX_OUTPUT_BYTES);
        assert!(state.truncated);
        assert_eq!(
            state
                .outputs
                .iter()
                .map(|chunk| chunk.bytes.len())
                .sum::<usize>(),
            HUMAN_SHELL_RUN_MAX_OUTPUT_BYTES
        );
    }

    async fn read_to_terminal(
        service: &HumanShellRunProtocol,
        owner: &str,
        session_id: &str,
        run_id: &str,
    ) -> (HumanShellRunSnapshotDto, Vec<u8>) {
        let mut cursor = 0;
        let mut output = Vec::new();
        for _ in 0..100 {
            match service.read(
                owner,
                Some(session_id),
                run_id,
                cursor,
                Some(HUMAN_SHELL_RUN_MAX_READ_BYTES),
            ) {
                CoreResponse::HumanShellRunRead {
                    snapshot, outputs, ..
                } => {
                    for chunk in outputs {
                        assert_eq!(chunk.sequence, cursor, "output sequence must be contiguous");
                        cursor = chunk.sequence.saturating_add(1);
                        output.extend_from_slice(&chunk.bytes);
                    }
                    if snapshot.status != HumanShellRunStatusDto::Running
                        && snapshot.status != HumanShellRunStatusDto::Starting
                    {
                        return (snapshot, output);
                    }
                }
                other => panic!("unexpected shell read response: {other:?}"),
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        panic!("shell run did not terminate");
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn two_workspaces_execute_on_owner_and_raw_output_is_owner_scoped() {
        let service = service();
        let left = tempfile::tempdir().unwrap();
        let right = tempfile::tempdir().unwrap();
        let left_request = request(
            "workspace-left",
            "session-left",
            "pwd; printf 'left-marker'",
        );
        let right_request = request(
            "workspace-right",
            "session-right",
            "pwd; printf 'right-marker'",
        );
        let left_response = service
            .create(
                "principal-a",
                &left_request,
                context(left.path(), "workspace-left", "session-left"),
            )
            .await;
        let right_response = service
            .create(
                "principal-b",
                &right_request,
                context(right.path(), "workspace-right", "session-right"),
            )
            .await;
        let (
            CoreResponse::HumanShellRunCreated {
                snapshot: left_run, ..
            },
            CoreResponse::HumanShellRunCreated {
                snapshot: right_run,
                ..
            },
        ) = (left_response, right_response)
        else {
            panic!("expected two created shell runs");
        };
        assert!(
            matches!(service.read("foreign", Some("session-left"), &left_run.run_id, 0, None), CoreResponse::Error { code, .. } if code == "human_shell_run_gone")
        );
        assert!(
            matches!(service.read("principal-a", Some("different-session"), &left_run.run_id, 0, None), CoreResponse::Error { code, .. } if code == "human_shell_run_gone")
        );
        let (left_done, left_output) =
            read_to_terminal(&service, "principal-a", "session-left", &left_run.run_id).await;
        let (right_done, right_output) =
            read_to_terminal(&service, "principal-b", "session-right", &right_run.run_id).await;
        assert_eq!(left_done.status, HumanShellRunStatusDto::Exited);
        assert_eq!(right_done.status, HumanShellRunStatusDto::Exited);
        let left_output = String::from_utf8_lossy(&left_output);
        let right_output = String::from_utf8_lossy(&right_output);
        assert!(left_output.contains(&left.path().display().to_string()));
        assert!(left_output.contains("left-marker"));
        assert!(!left_output.contains("right-marker"));
        assert!(right_output.contains(&right.path().display().to_string()));
        assert!(right_output.contains("right-marker"));
        assert!(!right_output.contains("left-marker"));
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn concurrent_creates_respect_per_principal_active_limit() {
        let mut config = crate::scheduler::config::ResolvedSchedulerConfig::default();
        config.resources.max_process_slots = 16;
        let service = HumanShellRunProtocol::new(Arc::new(AdmissionController::new(config)));
        let workspace = tempfile::tempdir().unwrap();
        let make = |index: usize| {
            let request = request("workspace-limit", &format!("session-{index}"), "sleep 5");
            let context = context(
                workspace.path(),
                "workspace-limit",
                &format!("session-{index}"),
            );
            (request, context)
        };
        let (r0, c0) = make(0);
        let (r1, c1) = make(1);
        let (r2, c2) = make(2);
        let (r3, c3) = make(3);
        let (r4, c4) = make(4);
        let (r5, c5) = make(5);
        let (a, b, c, d, e, f) = tokio::join!(
            service.create("same-principal", &r0, c0),
            service.create("same-principal", &r1, c1),
            service.create("same-principal", &r2, c2),
            service.create("same-principal", &r3, c3),
            service.create("same-principal", &r4, c4),
            service.create("same-principal", &r5, c5),
        );
        let responses = [a, b, c, d, e, f];
        let mut created = Vec::new();
        let mut rejected = 0;
        for (index, response) in responses.into_iter().enumerate() {
            match response {
                CoreResponse::HumanShellRunCreated { snapshot, .. } => {
                    created.push((snapshot.run_id, format!("session-{index}")))
                }
                CoreResponse::Error { code, .. } if code == "human_shell_run_limit" => {
                    rejected += 1
                }
                other => panic!("unexpected run-limit response: {other:?}"),
            }
        }
        assert_eq!(created.len(), HUMAN_SHELL_RUN_MAX_ACTIVE);
        assert_eq!(rejected, 6 - HUMAN_SHELL_RUN_MAX_ACTIVE);
        for (run_id, session_id) in created {
            let _ = service.cancel("same-principal", Some(&session_id), &run_id);
        }
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn dropping_daemon_protocol_kills_live_run_and_releases_permit() {
        let service = service();
        let admission = Arc::clone(&service.admission);
        let workspace = tempfile::tempdir().unwrap();
        let response = service
            .create(
                "shutdown-principal",
                &request("workspace-shutdown", "session-shutdown", "sleep 30"),
                context(workspace.path(), "workspace-shutdown", "session-shutdown"),
            )
            .await;
        assert!(matches!(
            response,
            CoreResponse::HumanShellRunCreated { .. }
        ));
        assert_eq!(admission.used_process_slots(), 1);

        drop(service);
        for _ in 0..100 {
            if admission.used_process_slots() == 0 {
                return;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        panic!("daemon shutdown left the human shell process admitted");
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn daemon_run_timeout_is_reported_as_timed_out() {
        let service = service();
        let workspace = tempfile::tempdir().unwrap();
        let mut request = request("workspace-timeout", "session-timeout", "sleep 5");
        request.timeout_secs = 1;
        let response = service
            .create(
                "timeout-principal",
                &request,
                context(workspace.path(), "workspace-timeout", "session-timeout"),
            )
            .await;
        let CoreResponse::HumanShellRunCreated { snapshot, .. } = response else {
            panic!("expected created shell run")
        };
        let (finished, _) = read_to_terminal(
            &service,
            "timeout-principal",
            "session-timeout",
            &snapshot.run_id,
        )
        .await;
        assert_eq!(finished.status, HumanShellRunStatusDto::TimedOut);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn cancellation_and_request_bounds_are_enforced() {
        let service = service();
        let workspace = tempfile::tempdir().unwrap();
        let invalid = request("workspace-cancel", "session-cancel", " ");
        assert!(
            matches!(service.create("principal", &invalid, context(workspace.path(), "workspace-cancel", "session-cancel")).await, CoreResponse::Error { code, .. } if code == "human_shell_invalid_request")
        );
        let long = request(
            "workspace-cancel",
            "session-cancel",
            &"x".repeat(crate::protocol::human_shell::HUMAN_SHELL_RUN_MAX_COMMAND_BYTES + 1),
        );
        assert!(!long.validate());
        let create = service
            .create(
                "principal",
                &request("workspace-cancel", "session-cancel", "sleep 5"),
                context(workspace.path(), "workspace-cancel", "session-cancel"),
            )
            .await;
        let CoreResponse::HumanShellRunCreated { snapshot, .. } = create else {
            panic!("expected created run")
        };
        assert!(
            matches!(service.read("principal", Some("session-cancel"), &snapshot.run_id, 0, Some(HUMAN_SHELL_RUN_MAX_READ_BYTES + 1)), CoreResponse::Error { code, .. } if code == "human_shell_invalid_request")
        );
        assert!(matches!(
            service.cancel("principal", Some("session-cancel"), &snapshot.run_id),
            CoreResponse::HumanShellRunCancelled { .. }
        ));
        let (done, _) =
            read_to_terminal(&service, "principal", "session-cancel", &snapshot.run_id).await;
        assert_eq!(done.status, HumanShellRunStatusDto::Cancelled);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn create_retry_is_idempotent_and_conflicting_reuse_is_refused() {
        let service = service();
        let workspace = tempfile::tempdir().unwrap();
        let request = request("workspace-retry", "session-retry", "sleep 5");
        let first = service
            .create(
                "principal",
                &request,
                context(workspace.path(), "workspace-retry", "session-retry"),
            )
            .await;
        let CoreResponse::HumanShellRunCreated {
            snapshot: first,
            duplicate: false,
        } = first
        else {
            panic!("expected first run create")
        };
        let retry = service
            .create(
                "principal",
                &request,
                context(workspace.path(), "workspace-retry", "session-retry"),
            )
            .await;
        let CoreResponse::HumanShellRunCreated {
            snapshot: retry,
            duplicate: true,
        } = retry
        else {
            panic!("expected idempotent create retry")
        };
        assert_eq!(first.run_id, retry.run_id);
        assert_eq!(service.admission.used_process_slots(), 1);
        let mut conflict = request.clone();
        conflict.command = "printf conflicting".to_owned();
        assert!(
            matches!(service.create("principal", &conflict, context(workspace.path(), "workspace-retry", "session-retry")).await, CoreResponse::Error { code, .. } if code == "human_shell_idempotency_conflict")
        );
        let _ = service.cancel("principal", Some("session-retry"), &first.run_id);
    }
}

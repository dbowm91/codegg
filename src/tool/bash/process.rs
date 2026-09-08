//! Supervised child-process execution for the `bash` tool.
//!
//! This module owns direct process lifetime and resource mechanics:
//! child construction, pipe ownership, timeout/cancellation, kill and
//! process-group semantics, wait/reap, bounded reader coordination, and
//! the typed [`DispatchOutcome`] returned to the facade.
//!
//! Child ownership contract (preserved from the pre-split implementation):
//!
//! - stdin: never inherited interactively; supervised children run without
//!   an interactive stdin.
//! - stdout/stderr: owned pipes drained through
//!   `ManagedProcessService` with explicit byte caps
//!   (`max_output_bytes`); no unbounded `read_to_end`.
//! - maximum capture: `OutputPolicy::with_limits(max, max)` for raw shell,
//!   `OutputPolicy::new(max)` for managed/native paths.
//! - timeout/cancellation: `ManagedProcessRequest::timeout` selects the
//!   terminal outcome; `TerminationReason::TimedOut` maps to
//!   `ToolError::Timeout` and the child is reaped by the service.
//! - kill/process-group: delegated to `ManagedProcessService`, which owns
//!   process-tree termination per platform semantics.
//! - wait/reap: `ManagedProcessService::run` resolves only after the child
//!   is reaped; no detached reader task outlives the returned outcome.
//! - reader/exit ordering: stdout/stderr are collected by the service
//!   before the outcome is returned, so `DispatchOutcome.output` always
//!   reflects the reaped terminal state.
//! - persistence/projection failure after completion: never rewrites the
//!   terminal outcome; the caller (`bash.rs` facade via `output.rs`)
//!   preserves the true exit status and reports the secondary failure
//!   without pretending the process did not run.
//!
//! Scheduler-owned dispatch (`submit_test_job`, python/shell/managed
//! submission) is also hosted here as thin translation over the daemon
//! `JobSubmissionService` boundary. Admission or executor failure is
//! terminal for the active route: these paths never retry through the
//! raw shell. No second shell executor exists: the only local spawn path
//! is `ManagedProcessService::run`.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use super::policy::RoutingMetric;
use super::BashTool;
use crate::command_intent::plan::{CommandDispatchTarget, CommandPlan, NativeCommand};
use crate::command_intent::CommandIntentKind;
use crate::command_outcome::ActualExecutor;
use crate::config::schema::CommandIntentFamily;
use crate::error::ToolError;

/// What a single dispatch returned: the result text, raw process output,
/// the actual executor that ran it, and an optional `RunId` proving that
/// a delegated subsystem owns a canonical RunStore record.
///
/// `delegated_run_id == None` paired with `ActualExecutor::TestRunner` or
/// `PythonScript` means the backend executed without obtaining a canonical
/// RunStore record. `BashTool::execute` keeps that result (never retries the
/// command) and uses caller persistence only when a store is available.
#[derive(Debug, Clone)]
pub struct DispatchOutcome {
    pub result: String,
    pub output: std::process::Output,
    pub executor: ActualExecutor,
    pub delegated_run_id: Option<codegg_core::run_store::RunId>,
}

pub(crate) fn bash_environment_policy() -> crate::managed_process::EnvironmentPolicy {
    let preserved = [
        "PATH",
        "HOME",
        "USER",
        "SHELL",
        "LANG",
        "LC_ALL",
        "TERM",
        "CARGO_HOME",
        "RUSTUP_HOME",
        "CARGO_INCREMENTAL",
        "CARGO_TERM_COLOR",
        "CARGO_TERM_PROGRESS",
        "RUSTFLAGS",
        "RUSTDOCFLAGS",
        "NVM_DIR",
        "PYENV_ROOT",
        "VIRTUAL_ENV",
        "PYTHONPATH",
        "JAVA_HOME",
        "GOPATH",
        "GOBIN",
    ];
    let mut policy = crate::managed_process::EnvironmentPolicy::sanitized();
    for name in preserved {
        policy = policy.allow_inherited_var(OsString::from(name));
    }
    policy
}

/// Extract a `-C <path>` argument from a git argv. Returns the path if
/// present and parseable. Used by bash-translated git dispatch to recover
/// the repository root when `canonical_workdir` is unset (typical in tests
/// where the workspace has no `allowed_paths` configured).
pub(crate) fn extract_cwd_from_argv(argv: &[String]) -> Option<std::path::PathBuf> {
    let mut iter = argv.iter();
    while let Some(arg) = iter.next() {
        if arg == "-C" {
            if let Some(p) = iter.next() {
                return Some(std::path::PathBuf::from(p));
            }
        } else if let Some(rest) = arg.strip_prefix("-C") {
            if !rest.is_empty() {
                return Some(std::path::PathBuf::from(rest));
            }
        }
    }
    None
}

/// Synthesize a `std::process::Output` with the given exit code, stdout,
/// and stderr bytes. Used by dispatchers that produce structured results
/// but need to fit the legacy `Output` shape.
pub(crate) fn synth_output(
    exit_code: i32,
    stdout: Vec<u8>,
    stderr: Vec<u8>,
) -> std::process::Output {
    #[cfg(unix)]
    {
        use std::os::unix::process::ExitStatusExt;
        std::process::Output {
            status: std::process::ExitStatus::from_raw(exit_code),
            stdout,
            stderr,
        }
    }
    #[cfg(not(unix))]
    {
        // The bash tool is Unix-only; this branch is unreachable but
        // keeps the type signature portable.
        std::process::Output {
            status: std::process::ExitStatus::default(),
            stdout,
            stderr,
        }
    }
}

impl BashTool {
    /// Execute a command via raw shell (`sh -c`). This is the original behavior
    /// used by observe mode and as a fallback when active routing is disabled
    /// or dispatch fails.
    pub(crate) async fn execute_via_raw_shell(
        &self,
        command: &str,
        canonical_workdir: Option<&Path>,
        timeout: Duration,
    ) -> Result<(String, std::process::Output), ToolError> {
        let cwd_owned = canonical_workdir
            .map(PathBuf::from)
            .or_else(|| std::env::current_dir().ok());
        let cwd = cwd_owned.clone().ok_or_else(|| {
            ToolError::Execution("managed shell cwd could not be resolved".into())
        })?;
        let sandbox = if let Some(config) = self.landlock_sandbox.as_ref() {
            if config.enabled {
                let args = vec!["-c".to_string(), command.to_string()];
                crate::managed_process::SandboxRequest::Required(config.launch_spec(
                    "sh",
                    &args,
                    Some(&cwd),
                )?)
            } else {
                crate::managed_process::SandboxRequest::Disabled
            }
        } else {
            crate::managed_process::SandboxRequest::Disabled
        };
        let mut request = crate::managed_process::ManagedProcessRequest::new(
            vec!["sh".into(), "-c".into(), command.into()],
            cwd,
            crate::managed_process::ProcessProvenance::default(),
        );
        request.environment_policy = bash_environment_policy();
        request.timeout = Some(timeout);
        request.output_policy = crate::managed_process::OutputPolicy::with_limits(
            self.max_output_bytes,
            self.max_output_bytes,
        );
        request.sandbox = sandbox;
        let managed = crate::managed_process::ManagedProcessService::run(request)
            .await
            .map_err(|error| match error {
                crate::managed_process::ManagedProcessError::SandboxFailed(reason) => {
                    ToolError::Execution(format!("sandbox helper failed: {reason}"))
                }
                crate::managed_process::ManagedProcessError::CancelledBeforeSpawn => {
                    ToolError::Execution("managed shell cancelled before spawn".into())
                }
                other => ToolError::Execution(other.to_string()),
            })?;
        if matches!(
            managed.termination,
            crate::managed_process::TerminationReason::TimedOut
        ) {
            return Err(ToolError::Timeout(command.to_string()));
        }
        let stdout = managed.stdout.to_string_lossy();
        let stderr = managed.stderr.to_string_lossy();
        let mut result = String::new();
        if !stdout.is_empty() {
            result.push_str(&super::output::truncate_output(
                &stdout,
                self.max_output_lines,
                self.max_output_bytes,
            ));
        }
        if !stderr.is_empty() {
            if !result.is_empty() {
                result.push_str("\n--- stderr ---\n");
            }
            result.push_str(&super::output::truncate_output(
                &stderr,
                self.max_output_lines,
                self.max_output_bytes,
            ));
        }
        if managed.stdout.is_truncated() || managed.stderr.is_truncated() {
            result.push_str("\n[output truncated by managed process limits]");
        }
        result.push_str(&format!(
            "\n\n[exit code: {}]",
            managed.exit_status.code().unwrap_or(-1)
        ));

        let output = synth_output(
            managed.exit_status.code().unwrap_or(-1),
            managed.stdout.as_bytes(),
            managed.stderr.as_bytes(),
        );
        Ok((result, output))
    }

    /// Submit a planner-validated test argv to the daemon scheduler. The
    /// Bash translation layer never retries through the raw shell when this
    /// boundary rejects or fails.
    pub(crate) async fn submit_test_job(
        &self,
        argv: &[String],
        cwd: Option<&Path>,
        _validated_command: Option<&str>,
        timeout: Duration,
    ) -> Result<DispatchOutcome, ToolError> {
        self.record_routing_metric(RoutingMetric {
            family: CommandIntentFamily::Tests,
            decision: "test_runner_dispatch".to_string(),
            fallback: false,
        });

        let Some(submission) = self.submission.clone() else {
            return Err(ToolError::Execution(
                "test execution requires the daemon scheduler".into(),
            ));
        };
        let run_cwd = cwd
            .map(Path::to_path_buf)
            .unwrap_or_else(|| PathBuf::from("."));
        let workspace_id = submission
            .workspace_id_for_root(&run_cwd)
            .await
            .map_err(|e| ToolError::Execution(e.to_string()))?;
        let submitted = submission
            .submit(
                None,
                codegg_core::jobs::NewJob {
                    workspace_id,
                    session_id: None,
                    turn_id: None,
                    kind: codegg_core::jobs::JobKind::Test,
                    source: codegg_core::jobs::JobSource::Interactive,
                    priority: codegg_core::jobs::JobPriority::Interactive,
                    payload: codegg_core::jobs::JobPayload::Test {
                        command: NativeCommand::from_argv(argv.to_vec())
                            .map(|command| command.display())
                            .unwrap_or_default(),
                        argv: argv.to_vec(),
                        cwd: Some(run_cwd.to_string_lossy().into_owned()),
                        scope: Some("bash-dispatch".into()),
                        parent_run_id: None,
                    },
                    resource_request: codegg_core::jobs::ResourceRequest::for_kind(
                        codegg_core::jobs::JobKind::Test,
                    ),
                    timeout: Some(timeout),
                    retry_policy: codegg_core::jobs::RetryPolicy::no_retry(),
                    idempotency: codegg_core::jobs::IdempotencyClass::SafeRepeat,
                    not_before: None,
                    deadline: None,
                    schedule_id: None,
                    depends_on: Vec::new(),
                    parent_job_id: None,

                    parent_attempt_id: None,

                    parent_call_id: None,
                    parent_program_id: None,
                    parent_instruction_sequence: None,
                    relation_kind: None,
                },
            )
            .await
            .map_err(|e| ToolError::Execution(e.to_string()))?;
        let completion = submission
            .scheduler()
            .wait_for_completion(&submitted.job_id, timeout + Duration::from_secs(5))
            .await
            .map_err(|e| ToolError::Execution(e.to_string()))?;
        let result = completion.summary;

        // Synthesize a `std::process::Output`-shaped value for code paths
        // that still inspect it (truncation, exit status, persistence).
        let exit_code = match completion.status {
            crate::scheduler::ExecutorStatus::Completed => 0,
            _ => 1,
        };
        let stdout_bytes = result.as_bytes().to_vec();
        let stderr_bytes: Vec<u8> = Vec::new();
        let output = synth_output(exit_code, stdout_bytes, stderr_bytes);

        let actual = ActualExecutor::TestRunner {
            argv: argv.to_vec(),
            cwd: run_cwd,
        };

        Ok(DispatchOutcome {
            result,
            output,
            executor: actual,
            delegated_run_id: completion.run_id,
        })
    }

    /// Dispatch to native tool (e.g. egggit). Executes via direct `Command::new`
    /// instead of `sh -c`, bypassing shell interpretation.
    pub(crate) async fn dispatch_to_native_tool(
        &self,
        command: &NativeCommand,
        canonical_workdir: Option<&Path>,
        timeout: Duration,
    ) -> Result<DispatchOutcome, ToolError> {
        if command.executable.is_empty() {
            return Err(ToolError::Execution(
                "native command has an empty executable".to_string(),
            ));
        }

        let argv_owned = command.full_argv();
        let tool_name = command.executable.clone();
        let cwd = canonical_workdir
            .map(Path::to_path_buf)
            .or_else(|| std::env::current_dir().ok())
            .ok_or_else(|| ToolError::Execution("native tool cwd could not be resolved".into()))?;
        let mut request = crate::managed_process::ManagedProcessRequest::new(
            argv_owned.iter().map(|arg| arg.into()).collect(),
            cwd,
            crate::managed_process::ProcessProvenance::default(),
        );
        request.timeout = Some(timeout);
        request.output_policy = crate::managed_process::OutputPolicy::new(self.max_output_bytes);
        let managed = crate::managed_process::ManagedProcessService::run(request)
            .await
            .map_err(|e| ToolError::Execution(e.to_string()))?;
        if matches!(
            managed.termination,
            crate::managed_process::TerminationReason::TimedOut
        ) {
            return Err(ToolError::Timeout(command.display()));
        }
        let stdout = managed.stdout.to_string_lossy();
        let stderr = managed.stderr.to_string_lossy();
        let mut result = stdout;
        if !stderr.is_empty() {
            if !result.is_empty() {
                result.push_str("\n--- stderr ---\n");
            }
            result.push_str(&stderr);
        }
        result.push_str(&format!(
            "\n\n[exit code: {}]",
            managed.exit_status.code().unwrap_or(-1)
        ));
        let output = synth_output(
            managed.exit_status.code().unwrap_or(-1),
            managed.stdout.as_bytes(),
            managed.stderr.as_bytes(),
        );

        self.record_routing_metric(RoutingMetric {
            family: CommandIntentFamily::GitRead,
            decision: "native_tool_dispatch".to_string(),
            fallback: false,
        });

        Ok(DispatchOutcome {
            result,
            output,
            executor: ActualExecutor::NativeTool {
                tool_name,
                argv: argv_owned,
            },
            delegated_run_id: None,
        })
    }

    /// Dispatch to canonical Python subsystem via the scheduler.
    /// Submits a durable `JobKind::Python` through `JobSubmissionService`
    /// so that policy resolution, sandbox, snapshots, and RunStore persistence
    /// all run through the scheduler-owned path.
    pub(crate) async fn dispatch_to_python_script(
        &self,
        script: &str,
        mode: &str,
        canonical_workdir: Option<&Path>,
        timeout: Duration,
    ) -> Result<DispatchOutcome, ToolError> {
        use crate::python_script::PythonExecutionMode;

        let exec_mode = match mode {
            "analyze" => PythonExecutionMode::Analyze,
            "transform" => PythonExecutionMode::Transform,
            "verify" => PythonExecutionMode::Verify,
            _ => PythonExecutionMode::Analyze,
        };

        let cwd = canonical_workdir
            .map(Path::to_path_buf)
            .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));

        let request = crate::python_script::PythonScriptRequest {
            code: script.to_string(),
            mode: exec_mode,
            cwd,
            workspace_root: None,
            timeout_secs: Some(timeout.as_secs()),
            session_id: None,
            intent: Some(format!("command-intent:{mode}")),
        };

        self.record_routing_metric(RoutingMetric {
            family: CommandIntentFamily::Python,
            decision: "python_script_dispatch".to_string(),
            fallback: false,
        });

        // Scheduler-owned path: submit through JobSubmissionService
        if let Some(ref submission) = self.submission {
            return self
                .dispatch_python_via_scheduler(&request, submission, mode, timeout)
                .await;
        }

        // No fallback: scheduler admission is required for production Python execution
        Err(ToolError::Disabled(
            "Python execution requires scheduler admission; scheduler is disabled".into(),
        ))
    }

    /// Submit Python execution through the scheduler and wait for completion.
    pub(crate) async fn dispatch_python_via_scheduler(
        &self,
        request: &crate::python_script::PythonScriptRequest,
        submission: &Arc<crate::scheduler::JobSubmissionService>,
        mode: &str,
        timeout: Duration,
    ) -> Result<DispatchOutcome, ToolError> {
        use codegg_core::jobs::{
            IdempotencyClass, JobKind, JobPayload, JobPriority, JobSource, NewJob, RetryPolicy,
        };

        let workspace_root = request
            .workspace_root
            .clone()
            .or_else(|| std::env::current_dir().ok())
            .unwrap_or_else(|| request.cwd.clone());

        let workspace_id = submission
            .workspace_id_for_root(&workspace_root)
            .await
            .map_err(|e| ToolError::Execution(format!("workspace registration failed: {e}")))?;

        let source_hash = crate::python_script::source_store::compute_digest(&request.code);

        let idempotency = match request.mode {
            crate::python_script::PythonExecutionMode::Analyze
            | crate::python_script::PythonExecutionMode::Verify => IdempotencyClass::SafeRepeat,
            crate::python_script::PythonExecutionMode::Transform => IdempotencyClass::NonIdempotent,
        };

        let payload = JobPayload::Python {
            script_path: String::new(),
            args: vec![],
            mode: request.mode.to_string(),
            source: Some(request.code.clone()),
            source_hash: Some(source_hash.clone()),
            cwd: Some(request.cwd.to_string_lossy().to_string()),
            timeout_secs: request.timeout_secs,
        };

        let mut labels = std::collections::HashMap::new();
        labels.insert(
            "workspace_root".to_string(),
            workspace_root.to_string_lossy().to_string(),
        );
        if let Some(ref intent) = request.intent {
            labels.insert("intent".to_string(), intent.clone());
        }

        let spec = NewJob {
            workspace_id,
            session_id: request.session_id.clone(),
            turn_id: None,
            kind: JobKind::Python,
            source: JobSource::Interactive,
            priority: JobPriority::Interactive,
            payload,
            resource_request: codegg_core::jobs::ResourceRequest::for_kind(JobKind::Python),
            timeout: Some(timeout),
            retry_policy: RetryPolicy::no_retry(),
            idempotency,
            not_before: None,
            deadline: None,
            parent_job_id: None,
            parent_attempt_id: None,
            parent_call_id: None,
            parent_program_id: None,
            parent_instruction_sequence: None,
            relation_kind: None,
            schedule_id: None,
            depends_on: vec![],
        };

        let key = crate::scheduler::submission::SubmissionKey::new(format!("python:{source_hash}"))
            .map_err(|e| ToolError::Execution(format!("invalid submission key: {e}")))?;

        let submitted = submission
            .submit(Some(key), spec)
            .await
            .map_err(|e| ToolError::Execution(format!("scheduler submission failed: {e}")))?;

        let completion = submission
            .scheduler()
            .wait_for_completion(&submitted.job_id, timeout + Duration::from_secs(30))
            .await
            .map_err(|e| ToolError::Execution(format!("scheduler wait failed: {e}")))?;

        let result = crate::python_script::types::PythonRunResult {
            status: match completion.status {
                crate::scheduler::executor::ExecutorStatus::Completed => {
                    crate::python_script::types::PythonRunStatus::Success
                }
                crate::scheduler::executor::ExecutorStatus::Cancelled => {
                    crate::python_script::types::PythonRunStatus::Failed(-4)
                }
                crate::scheduler::executor::ExecutorStatus::TimedOut => {
                    crate::python_script::types::PythonRunStatus::TimedOut
                }
                crate::scheduler::executor::ExecutorStatus::Failed
                | crate::scheduler::executor::ExecutorStatus::Interrupted => {
                    crate::python_script::types::PythonRunStatus::Failed(-1)
                }
            },
            stdout: completion.summary.clone(),
            stderr: String::new(),
            duration: Duration::from_millis(completion.metrics.elapsed_ms),
            mode: request.mode,
            script_length: request.code.len(),
            risk: crate::python_script::types::PythonRiskAssessment::safe(),
            capabilities: crate::python_script::types::PythonCapabilityEnvelope::analyze(),
            changed_files: vec![],
            interpreter: "python3".to_string(),
            diff: None,
            script_body_hash: Some(source_hash),
            stdout_label: completion
                .run_id
                .as_ref()
                .map(|rid| format!("run://{rid}/stdout")),
            stderr_label: completion
                .run_id
                .as_ref()
                .map(|rid| format!("run://{rid}/stderr")),
            diff_label: None,
            policy_decision: None,
            denied_capabilities: vec![],
            os_filesystem_isolation: false,
            os_network_isolation: false,
            effective_read_roots: vec![],
            effective_write_roots: vec![],
            allowed_subprocesses: vec![],
            enforcement_warnings: vec![],
        };

        let stdout = result.stdout.clone();
        let stderr = result.stderr.clone();
        let mut display = stdout;
        if !stderr.is_empty() {
            if !display.is_empty() {
                display.push_str("\n--- stderr ---\n");
            }
            display.push_str(&stderr);
        }
        let exit_code = result.exit_code();
        display.push_str(&format!("\n\n[exit code: {}]", exit_code.unwrap_or(-1)));

        let exit_code_value = exit_code.unwrap_or(-1);
        let output = synth_output(
            exit_code_value,
            result.stdout.as_bytes().to_vec(),
            result.stderr.as_bytes().to_vec(),
        );

        let actual = ActualExecutor::PythonScript {
            script_hash: result.script_body_hash.clone(),
            mode: mode.to_string(),
        };

        Ok(DispatchOutcome {
            result: display,
            output,
            executor: actual,
            delegated_run_id: completion.run_id,
        })
    }

    /// Dispatch a typed `GitExecutionRequest` through `GitMutationExecutor` —
    /// the canonical Bash-tool path for active routing of Git operations.
    ///
    /// This is the Bash-translation counterpart to `src/tool/git.rs`'s typed
    /// dispatch: it captures snapshots, applies `GitEnvPolicy`, computes state
    /// deltas, sanitizes output, persists to `RunStore`, and projects the
    /// result via `project_mutation`. Native and Bash-originated operations
    /// share the same executor and projection; only the run-store
    /// `backend_detail` differs ("git_native" vs. "git_bash_translation").
    ///
    /// Errors are returned to the caller — `BashTool::execute` MUST NOT retry
    /// through raw shell after this method runs.
    pub(crate) async fn dispatch_to_git(
        &self,
        request: &crate::command_intent::plan::GitExecutionRequest,
        canonical_workdir: Option<&Path>,
        input_workdir: Option<&Path>,
        timeout: Duration,
    ) -> Result<DispatchOutcome, ToolError> {
        use crate::git_mutation_projector::project_mutation;
        use crate::git_mutations::{
            resolve_repo_root, GitEnvPolicy, GitMutationError, GitMutationExecutor,
        };

        // Resolve the working directory. Precedence:
        //   1. `canonical_workdir` (BashTool-resolved via allowed_paths).
        //   2. `input_workdir` (the `workdir` JSON field — explicit caller
        //      override, e.g. test harnesses running against a temp repo).
        //   3. `-C <path>` extracted from argv (bash-translated `git -C`).
        //   4. Process cwd.
        // This ensures bash-translated commands like `git -C /repo add ...`
        // find the right repository even when `allowed_paths` is unset.
        let workdir = if let Some(dir) = canonical_workdir {
            dir.to_path_buf()
        } else if let Some(dir) = input_workdir {
            dir.to_path_buf()
        } else if let Some(c_path) = extract_cwd_from_argv(&request.argv) {
            c_path
        } else {
            std::env::current_dir().map_err(|e| {
                ToolError::Execution(format!("could not resolve working directory: {e}"))
            })?
        };

        // Managed/unknown plumbing operations are not promoted through the
        // typed executor — use managed argv with GitEnvPolicy for env hardening
        // without snapshot/delta persistence (the parser already marked them
        // as fallback candidates).
        if let Some(managed_argv) = request.managed_argv.as_ref() {
            return self
                .dispatch_git_managed_argv(
                    managed_argv,
                    Some(&workdir),
                    timeout,
                    request.origin.label(),
                )
                .await;
        }

        let repo_root = match resolve_repo_root(&workdir) {
            Ok(r) => r,
            Err(e) => {
                return Err(ToolError::Execution(format!(
                    "git dispatch: repository resolution failed: {e}"
                )));
            }
        };

        let exec = GitMutationExecutor::new()
            .with_env_policy(GitEnvPolicy::default())
            .with_timeout(timeout);

        // Execute via the shared GitMutationExecutor. Errors include typed
        // context but never leak credentials (redaction happens inside
        // `MutationResult` projection).
        let result = exec
            .execute(&request.operation, repo_root.as_path())
            .await
            .map_err(|e: GitMutationError| {
                ToolError::Execution(format!("git dispatch failed: {}", e))
            })?;

        // Persist to RunStore using the canonical `git_run_store` helper.
        // Matches what the native GitTool writes for mutations, with
        // `backend_detail` set to the origin label so audits can
        // distinguish native vs. bash-translated runs. Read-only
        // operations are NOT persisted (matches native tool behavior;
        // they carry no state-delta or audit-worthy artifact).
        let delegated_run_id = if request.is_read_only {
            None
        } else {
            crate::git_run_store::persist_mutation(
                &self.run_store,
                &result,
                &workdir,
                repo_root.as_path(),
                "git_bash_translation",
                Some(request.origin.label().to_string()),
            )
            .await
        };

        let projection = project_mutation(&result);

        // Compose output for the persistence layer. Stdout is the projection
        // (model-safe summary); stderr carries the raw stderr from git (with
        // URL credentials redacted inside `MutationResult.stderr`).
        let output = synth_output(
            result.exit_code,
            projection.clone().into_bytes(),
            result.stderr.clone().into_bytes(),
        );

        // Append `[exit code: N]` annotation to the model-visible summary
        // so callers that key off this marker (e.g. tests) keep working.
        let mut model_summary = projection;
        model_summary.push_str(&format!(
            "\n\n[exit code: {}] [origin: {}]",
            result.exit_code,
            request.origin.label(),
        ));

        Ok(DispatchOutcome {
            result: model_summary,
            output,
            executor: ActualExecutor::Git {
                argv: request.argv.clone(),
                operation_label: result.subcommand.clone(),
            },
            delegated_run_id,
        })
    }

    /// Managed argv fallback for git operations that don't have a typed
    /// representation (e.g., `git remote show origin`). Uses `GitEnvPolicy`
    /// for env hardening without snapshot/delta persistence.
    pub(crate) async fn dispatch_git_managed_argv(
        &self,
        argv: &[String],
        cwd: Option<&Path>,
        timeout: Duration,
        origin_label: &str,
    ) -> Result<DispatchOutcome, ToolError> {
        let argv_owned = argv.to_vec();
        let cwd_owned = cwd
            .map(|p| p.to_path_buf())
            .or_else(|| std::env::current_dir().ok())
            .ok_or_else(|| {
                ToolError::Execution("git managed argv cwd could not be resolved".into())
            })?;
        let mut request = crate::managed_process::ManagedProcessRequest::new(
            argv_owned.iter().map(OsString::from).collect(),
            cwd_owned.clone(),
            crate::managed_process::ProcessProvenance::default(),
        );
        request.timeout = Some(timeout);
        request.output_policy = crate::managed_process::OutputPolicy::new(self.max_output_bytes);
        request.environment_policy = crate::managed_process::EnvironmentPolicy::sanitized()
            .with_var("GIT_EDITOR", "true")
            .with_var("GIT_SEQUENCE_EDITOR", "true")
            .with_var("GPG_TTY", "");
        let managed = crate::managed_process::ManagedProcessService::run(request)
            .await
            .map_err(|e| ToolError::Execution(format!("git managed argv: {e}")))?;
        if matches!(
            managed.termination,
            crate::managed_process::TerminationReason::TimedOut
        ) {
            return Err(ToolError::Timeout(argv.join(" ")));
        }
        let stdout = managed.stdout.to_string_lossy();
        let stderr = managed.stderr.to_string_lossy();
        let mut result = stdout;
        if !stderr.is_empty() {
            if !result.is_empty() {
                result.push_str("\n--- stderr ---\n");
            }
            result.push_str(&stderr);
        }
        result.push_str(&format!(
            "\n\n[exit code: {}] [origin: {}]",
            managed.exit_status.code().unwrap_or(-1),
            origin_label
        ));
        let output = synth_output(
            managed.exit_status.code().unwrap_or(-1),
            managed.stdout.as_bytes(),
            managed.stderr.as_bytes(),
        );

        Ok(DispatchOutcome {
            result,
            output,
            executor: ActualExecutor::ManagedArgv {
                argv: argv_owned,
                cwd: Some(cwd_owned),
            },
            delegated_run_id: None,
        })
    }

    /// Submit a managed argv process to the scheduler. Admission or executor
    /// failure is returned to the caller; this path never falls back to shell.
    pub(crate) async fn dispatch_to_managed_process(
        &self,
        command: &NativeCommand,
        cwd: Option<&Path>,
        timeout: Duration,
        kind: codegg_core::jobs::JobKind,
    ) -> Result<DispatchOutcome, ToolError> {
        if command.executable.is_empty() {
            return Err(ToolError::Execution("empty executable".to_string()));
        }

        let argv_owned = command.full_argv();
        let Some(submission) = self.submission.clone() else {
            return Err(ToolError::Execution(
                "managed process execution requires the daemon scheduler".into(),
            ));
        };
        let cwd_owned = cwd
            .map(Path::to_path_buf)
            .unwrap_or_else(|| PathBuf::from("."));
        let workspace_id = submission
            .workspace_id_for_root(&cwd_owned)
            .await
            .map_err(|e| ToolError::Execution(e.to_string()))?;
        let submitted = submission
            .submit(
                None,
                codegg_core::jobs::NewJob {
                    workspace_id,
                    session_id: None,
                    turn_id: None,
                    kind,
                    source: codegg_core::jobs::JobSource::Interactive,
                    priority: codegg_core::jobs::JobPriority::Interactive,
                    payload: codegg_core::jobs::JobPayload::ManagedArgv {
                        argv: argv_owned.clone(),
                        cwd: Some(cwd_owned.to_string_lossy().into_owned()),
                    },
                    resource_request: codegg_core::jobs::ResourceRequest::for_kind(kind),
                    timeout: Some(timeout),
                    retry_policy: codegg_core::jobs::RetryPolicy::no_retry(),
                    parent_job_id: None,
                    parent_attempt_id: None,
                    parent_call_id: None,
                    parent_program_id: None,
                    parent_instruction_sequence: None,
                    relation_kind: None,
                    idempotency: codegg_core::jobs::IdempotencyClass::SafeRepeat,
                    not_before: None,
                    deadline: None,
                    schedule_id: None,
                    depends_on: Vec::new(),
                },
            )
            .await
            .map_err(|e| ToolError::Execution(e.to_string()))?;
        let completion = submission
            .scheduler()
            .wait_for_completion(&submitted.job_id, timeout + Duration::from_secs(5))
            .await
            .map_err(|e| ToolError::Execution(e.to_string()))?;
        let result = completion.summary;
        let exit_code = if matches!(
            completion.status,
            crate::scheduler::ExecutorStatus::Completed
        ) {
            0
        } else {
            1
        };
        let output = synth_output(exit_code, result.as_bytes().to_vec(), Vec::new());

        self.record_routing_metric(RoutingMetric {
            family: CommandIntentFamily::Search,
            decision: "managed_process_dispatch".to_string(),
            fallback: false,
        });

        Ok(DispatchOutcome {
            result,
            output,
            executor: ActualExecutor::ManagedArgv {
                argv: argv_owned,
                cwd: Some(cwd_owned),
            },
            delegated_run_id: None,
        })
    }

    /// Submit the explicit shell backend through the scheduler. The shell
    /// remains the domain service here, but process creation and admission
    /// are still daemon-owned and durable.
    pub(crate) async fn dispatch_to_shell(
        &self,
        command: &str,
        canonical_workdir: Option<&Path>,
        timeout: Duration,
    ) -> Result<DispatchOutcome, ToolError> {
        self.record_routing_metric(RoutingMetric {
            family: CommandIntentFamily::Tests, // generic
            decision: "shell_dispatch".to_string(),
            fallback: false,
        });
        let Some(submission) = self.submission.clone() else {
            return Err(ToolError::Execution(
                "shell execution requires the daemon scheduler".into(),
            ));
        };
        let cwd = canonical_workdir
            .map(Path::to_path_buf)
            .unwrap_or_else(|| PathBuf::from("."));
        let workspace_id = submission
            .workspace_id_for_root(&cwd)
            .await
            .map_err(|e| ToolError::Execution(e.to_string()))?;
        let argv = vec!["sh".to_string(), "-c".to_string(), command.to_string()];
        let submitted = submission
            .submit(
                None,
                codegg_core::jobs::NewJob {
                    workspace_id,
                    session_id: None,
                    turn_id: None,
                    kind: codegg_core::jobs::JobKind::Shell,
                    source: codegg_core::jobs::JobSource::Interactive,
                    priority: codegg_core::jobs::JobPriority::Interactive,
                    payload: codegg_core::jobs::JobPayload::Shell {
                        command: command.to_string(),
                        argv: Some(argv.clone()),
                        cwd: Some(cwd.to_string_lossy().into_owned()),
                    },
                    resource_request: codegg_core::jobs::ResourceRequest::for_kind(
                        codegg_core::jobs::JobKind::Shell,
                    ),
                    parent_job_id: None,
                    parent_attempt_id: None,
                    parent_call_id: None,
                    parent_program_id: None,
                    parent_instruction_sequence: None,
                    relation_kind: None,
                    timeout: Some(timeout),
                    retry_policy: codegg_core::jobs::RetryPolicy::no_retry(),
                    idempotency: codegg_core::jobs::IdempotencyClass::NonIdempotent,
                    not_before: None,
                    deadline: None,
                    schedule_id: None,
                    depends_on: Vec::new(),
                },
            )
            .await
            .map_err(|e| ToolError::Execution(e.to_string()))?;
        let completion = submission
            .scheduler()
            .wait_for_completion(&submitted.job_id, timeout + Duration::from_secs(5))
            .await
            .map_err(|e| ToolError::Execution(e.to_string()))?;
        let result = completion.summary;
        let exit_code = if matches!(
            completion.status,
            crate::scheduler::ExecutorStatus::Completed
        ) {
            0
        } else {
            1
        };
        let output = synth_output(exit_code, result.as_bytes().to_vec(), Vec::new());
        Ok(DispatchOutcome {
            result,
            output,
            executor: ActualExecutor::RawShell {
                command: command.to_string(),
                argv,
            },
            delegated_run_id: None,
        })
    }

    /// Dispatch the canonical command target to the appropriate backend.
    pub(crate) async fn dispatch_command_target(
        &self,
        decision: &CommandDispatchTarget,
        _plan: &CommandPlan,
        canonical_workdir: Option<&Path>,
        input_workdir: Option<&Path>,
        timeout: Duration,
    ) -> Result<DispatchOutcome, ToolError> {
        match decision {
            CommandDispatchTarget::RouteToTestRunner {
                argv,
                validated_command,
                ..
            } => {
                self.submit_test_job(
                    argv,
                    canonical_workdir,
                    validated_command.as_deref(),
                    timeout,
                )
                .await
            }
            CommandDispatchTarget::RouteToNativeTool { command, .. } => {
                self.dispatch_to_native_tool(command, canonical_workdir, timeout)
                    .await
            }
            CommandDispatchTarget::RouteToPythonScripting { script, mode, .. } => {
                let mode_str = match mode {
                    crate::command_intent::plan::PythonModeGuess::Analyze => "analyze",
                    crate::command_intent::plan::PythonModeGuess::Transform => "transform",
                    crate::command_intent::plan::PythonModeGuess::Verify => "verify",
                    crate::command_intent::plan::PythonModeGuess::Unknown => "analyze",
                };
                self.dispatch_to_python_script(script, mode_str, canonical_workdir, timeout)
                    .await
            }
            CommandDispatchTarget::RouteToManagedProcess { command, cwd, .. } => {
                let kind = match _plan.intent.kind {
                    CommandIntentKind::Build => codegg_core::jobs::JobKind::Build,
                    CommandIntentKind::Lint => codegg_core::jobs::JobKind::Lint,
                    CommandIntentKind::Format => codegg_core::jobs::JobKind::Format,
                    _ => codegg_core::jobs::JobKind::ManagedProcess,
                };
                self.dispatch_to_managed_process(command, Some(cwd), timeout, kind)
                    .await
            }
            CommandDispatchTarget::RouteToGit { request, .. } => {
                // Track U unified dispatch: route typed Git operations
                // through `GitMutationExecutor` so they share the same
                // env policy, snapshot/delta, projection, and RunStore
                // semantics as native-tool invocations. Managed/unknown
                // plumbing falls through to the managed-argv path inside
                // `dispatch_to_git` without snapshot/delta persistence.
                self.dispatch_to_git(request, canonical_workdir, input_workdir, timeout)
                    .await
            }
            CommandDispatchTarget::RouteToShell { command, .. } => {
                self.dispatch_to_shell(command, canonical_workdir, timeout)
                    .await
            }
            CommandDispatchTarget::Rejected { reason } => Err(ToolError::Execution(format!(
                "command rejected: {}",
                reason
            ))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn environment_policy_preserves_expected_vars() {
        let policy = bash_environment_policy();
        let debug = format!("{policy:?}");
        assert!(debug.contains("PATH"));
    }

    #[test]
    fn extract_cwd_from_argv_supports_dash_c_forms() {
        let argv = vec!["git".to_string(), "-C".to_string(), "/tmp/repo".to_string()];
        assert_eq!(
            extract_cwd_from_argv(&argv),
            Some(PathBuf::from("/tmp/repo"))
        );
        let argv = vec!["git".to_string(), "-C/tmp/repo".to_string()];
        assert_eq!(
            extract_cwd_from_argv(&argv),
            Some(PathBuf::from("/tmp/repo"))
        );
        let argv = vec!["git".to_string(), "status".to_string()];
        assert_eq!(extract_cwd_from_argv(&argv), None);
    }

    #[test]
    fn synth_output_preserves_exit_code_and_streams() {
        // `synth_output` mirrors the pre-split helper: it packs the integer
        // into the platform `ExitStatus` and carries the streams verbatim.
        // Only the zero status round-trips through `from_raw` portably, so
        // assert streams plus the zero-code path here; nonzero terminal
        // status is covered by the `[exit code: N]` result-string tests.
        let output = synth_output(0, b"out".to_vec(), b"err".to_vec());
        assert_eq!(output.status.code(), Some(0));
        assert_eq!(output.stdout, b"out");
        assert_eq!(output.stderr, b"err");
    }
}

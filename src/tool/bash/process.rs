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

/// Bytes of partial output carried into a timeout result.
///
/// The service already caps capture at `max_output_bytes`; this is a second,
/// smaller bound so a timeout message cannot swamp the model's context with
/// a runaway command's worth of output.
const TIMEOUT_OUTPUT_BYTES: usize = 4000;

#[derive(Debug, Clone, Copy)]
pub(crate) struct RawShellResultMetadata {
    pub exit_code: Option<i32>,
    pub output_truncated: bool,
    pub cleanup: &'static str,
    pub helper: &'static str,
}

/// Tail `data` to [`TIMEOUT_OUTPUT_BYTES`], marking that it was truncated.
///
/// The tail is kept rather than the head: on a timeout the most recent
/// output is what identifies where the command actually stopped.
fn tail_bounded(data: &str) -> (String, bool) {
    if data.len() <= TIMEOUT_OUTPUT_BYTES {
        return (data.to_string(), false);
    }
    let start = data.len() - TIMEOUT_OUTPUT_BYTES;
    // Walk back to a char boundary so multi-byte output cannot panic.
    let mut start = start;
    while start < data.len() && !data.is_char_boundary(start) {
        start += 1;
    }
    let mut out = String::with_capacity(TIMEOUT_OUTPUT_BYTES + 64);
    out.push_str("...[earlier output truncated]...\n");
    out.push_str(&data[start..]);
    (out, true)
}

/// Build the timeout result text for a killed child, keeping what it printed.
///
/// Returning the command name alone tells the model only that it ran too
/// long. The captured streams are the evidence, and they are already
/// bounded by the process service's output policy.
fn partial_timeout_report(command: &str, stdout: &str, stderr: &str) -> String {
    let mut msg = format!("`{command}` timed out and was terminated.");
    let (out, out_trunc) = tail_bounded(stdout);
    let (err, err_trunc) = tail_bounded(stderr);
    if !out.trim().is_empty() {
        msg.push_str("\n\n--- stdout before termination ---\n");
        msg.push_str(&out);
    }
    if !err.trim().is_empty() {
        msg.push_str("\n\n--- stderr before termination ---\n");
        msg.push_str(&err);
    }
    if out_trunc || err_trunc {
        msg.push_str("\n\n[partial output: tail retained, earlier bytes dropped]");
    }
    if out.trim().is_empty() && err.trim().is_empty() {
        msg.push_str("\n\n[no output was produced before termination]");
    }
    msg
}

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
        "USERPROFILE",
        "APPDATA",
        "LOCALAPPDATA",
        "PROGRAMDATA",
        "PROGRAMFILES",
        "PROGRAMFILES(X86)",
        "PATHEXT",
        "COMSPEC",
        "PSModulePath",
    ];
    let mut policy = crate::managed_process::EnvironmentPolicy::sanitized();
    for name in preserved {
        policy = policy.allow_inherited_var(OsString::from(name));
    }
    policy
}

/// Build a shell invocation without flattening argv into a command line.
/// Shell parsing is intentionally limited to raw shell routes; structured
/// `NativeCommand` and managed argv routes continue to spawn their executable
/// directly.
pub(crate) fn shell_argv(
    command: &str,
    configured: Option<crate::config::schema::WindowsShellKind>,
) -> Result<Vec<String>, ToolError> {
    #[cfg(windows)]
    {
        let system_root = std::env::var_os("SystemRoot")
            .or_else(|| std::env::var_os("WINDIR"))
            .filter(|value| !value.is_empty())
            .map(PathBuf::from)
            .ok_or_else(|| ToolError::Execution("Windows SystemRoot is unavailable".into()))?;
        let (executable, args) =
            match configured.unwrap_or(crate::config::schema::WindowsShellKind::PowerShell) {
                crate::config::schema::WindowsShellKind::PowerShell => (
                    system_root.join("System32/WindowsPowerShell/v1.0/powershell.exe"),
                    vec![
                        "-NoLogo".to_string(),
                        "-NoProfile".to_string(),
                        "-NonInteractive".to_string(),
                        "-Command".to_string(),
                        command.to_string(),
                    ],
                ),
                crate::config::schema::WindowsShellKind::Cmd => (
                    system_root.join("System32").join("cmd.exe"),
                    vec![
                        "/D".to_string(),
                        "/S".to_string(),
                        "/C".to_string(),
                        command.to_string(),
                    ],
                ),
                crate::config::schema::WindowsShellKind::GitBash => {
                    let executable = ["ProgramFiles", "ProgramFiles(x86)"]
                        .into_iter()
                        .filter_map(std::env::var_os)
                        .filter(|value| !value.is_empty())
                        .map(PathBuf::from)
                        .map(|root| root.join("Git/bin/bash.exe"))
                        .find(|candidate| candidate.is_file())
                        .ok_or_else(|| {
                            ToolError::Execution("Git Bash was not found in ProgramFiles".into())
                        })?;
                    (executable, vec!["-c".to_string(), command.to_string()])
                }
                crate::config::schema::WindowsShellKind::Wsl => (
                    system_root.join("System32/wsl.exe"),
                    vec![
                        "--exec".to_string(),
                        "sh".to_string(),
                        "-c".to_string(),
                        command.to_string(),
                    ],
                ),
            };
        if !executable.is_file() {
            return Err(ToolError::Execution(format!(
                "configured Windows shell is unavailable: {}",
                executable.display()
            )));
        }
        let mut argv = Vec::with_capacity(args.len() + 1);
        argv.push(executable.to_string_lossy().into_owned());
        argv.extend(args);
        Ok(argv)
    }
    #[cfg(not(windows))]
    {
        let _ = configured;
        Ok(vec![
            "sh".to_string(),
            "-c".to_string(),
            command.to_string(),
        ])
    }
}

/// Move a cmd.exe shell string out of argv so the process owner can append it
/// with Windows' raw command-line API. `cmd.exe` does not use the standard C
/// runtime argv grammar, so quoting the string as an ordinary argument changes
/// its shell meaning.
#[cfg(windows)]
pub(crate) fn extract_cmd_raw_command(
    command: &str,
    argv: &mut Vec<String>,
) -> Result<Option<OsString>, &'static str> {
    let is_cmd = argv
        .first()
        .and_then(|executable| Path::new(executable).file_name())
        .and_then(|name| name.to_str())
        .is_some_and(|name| name.eq_ignore_ascii_case("cmd.exe"));
    if !is_cmd {
        return Ok(None);
    }
    if argv.last().is_none_or(|argument| argument != command) {
        return Err("cmd shell payload is missing its raw command argument");
    }
    argv.pop();
    // Rust's Windows raw-argument API appends this as one literal command-line
    // segment. `/S` makes cmd.exe strip exactly the outer quote pair around its
    // `/C` command string, leaving the user shell text intact.
    Ok(Some(OsString::from(format!("\"{command}\""))))
}

#[cfg(windows)]
pub(crate) fn create_shell_temp_dir() -> Result<tempfile::TempDir, ToolError> {
    tempfile::Builder::new()
        .prefix("codegg-shell-")
        .tempdir()
        .map_err(|error| ToolError::Execution(format!("create isolated shell temp dir: {error}")))
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
    #[cfg(windows)]
    pub(crate) async fn execute_via_raw_shell(
        &self,
        command: &str,
        canonical_workdir: Option<&Path>,
        timeout: Duration,
    ) -> Result<(String, std::process::Output), ToolError> {
        let (result, output, _, _) = self
            .execute_via_raw_shell_with_authorization(
                command,
                canonical_workdir,
                timeout,
                None,
                None,
            )
            .await?;
        Ok((result, output))
    }

    pub(crate) async fn execute_via_raw_shell_with_authorization(
        &self,
        command: &str,
        canonical_workdir: Option<&Path>,
        timeout: Duration,
        authorization: Option<crate::tool::authorization::ExecutionAuthorizationContext>,
        session_id: Option<&str>,
    ) -> Result<
        (
            String,
            std::process::Output,
            crate::managed_process::SandboxExecutionOutcome,
            RawShellResultMetadata,
        ),
        ToolError,
    > {
        let cwd_owned = canonical_workdir
            .map(PathBuf::from)
            .or_else(|| std::env::current_dir().ok());
        let cwd = cwd_owned.clone().ok_or_else(|| {
            ToolError::Execution("managed shell cwd could not be resolved".into())
        })?;
        let raw_shell_argv = shell_argv(command, self.windows_shell)?;
        // One place decides contained vs unavailable vs unconstrained, from
        // the real host capability. A constrained command never launches
        // through a raw shell when the helper cannot enforce its policy, and
        // an unavailable path is never reported as containment or as FullHost.
        let sandbox_path =
            crate::security::sandbox::sandbox_execution_path(self.landlock_sandbox.as_ref());
        let sandbox = match &sandbox_path {
            crate::security::sandbox::SandboxExecutionPath::Unconstrained => {
                crate::managed_process::SandboxRequest::Disabled
            }
            crate::security::sandbox::SandboxExecutionPath::Contained { .. } => {
                let config = self
                    .landlock_sandbox
                    .as_ref()
                    .expect("contained path requires an enabled config");
                let args = raw_shell_argv[1..].to_vec();
                crate::managed_process::SandboxRequest::Required(config.launch_spec(
                    &raw_shell_argv[0],
                    &args,
                    Some(&cwd),
                )?)
            }
            crate::security::sandbox::SandboxExecutionPath::Unavailable { reason } => {
                return Err(ToolError::Execution(format!(
                    "requested filesystem containment is unavailable; command refused: {reason}"
                )));
            }
        };
        let managed_shell_argv = raw_shell_argv.clone();
        #[cfg(windows)]
        let mut managed_shell_argv = managed_shell_argv;
        #[cfg(windows)]
        let raw_cmd_command = extract_cmd_raw_command(command, &mut managed_shell_argv)
            .map_err(|error| ToolError::Execution(error.into()))?;
        let mut request = crate::managed_process::ManagedProcessRequest::new(
            managed_shell_argv.into_iter().map(OsString::from).collect(),
            cwd,
            session_id.map_or_else(
                crate::managed_process::ProcessProvenance::default,
                |session_id| {
                    crate::managed_process::ProcessProvenance::default()
                        .with_session(session_id.to_owned())
                },
            ),
        );
        request.execution_authorization = authorization;
        #[cfg(windows)]
        if let Some(raw_command) = raw_cmd_command {
            request = request.with_raw_command(raw_command);
        }
        request.environment_policy = bash_environment_policy();
        #[cfg(windows)]
        if matches!(
            self.windows_shell,
            Some(crate::config::schema::WindowsShellKind::Cmd)
        ) {
            request.environment_policy = request
                .environment_policy
                .clone()
                .with_var(OsString::from("COMSPEC"), request.executable.clone());
        }
        #[cfg(windows)]
        let _shell_temp = {
            let directory = create_shell_temp_dir()?;
            let path = directory.path().as_os_str().to_owned();
            request.environment_policy = request
                .environment_policy
                .clone()
                .with_var(OsString::from("TEMP"), path.clone())
                .with_var(OsString::from("TMP"), path);
            directory
        };
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
            // The child was killed but its pipes were already drained, so
            // whatever it managed to print is available here. Discarding
            // it would throw away the only clue about *why* a run went
            // long — which half of a build failed, which test hung — and
            // force the model to re-run blind to find out.
            return Err(ToolError::Timeout(partial_timeout_report(
                command,
                &managed.stdout.to_string_lossy(),
                &managed.stderr.to_string_lossy(),
            )));
        }
        let stdout = managed.stdout.to_string_lossy();
        let stderr = managed.stderr.to_string_lossy();
        let mut result = String::new();
        match &managed.sandbox {
            #[cfg(all(windows, feature = "windows-sandbox-test-support"))]
            crate::managed_process::SandboxExecutionOutcome::TestOnlyAppContainer {
                status,
                ..
            } => {
                result.push_str(&format!(
                    "[sandbox: TEST-ONLY AppContainer process control; filesystem rights are not enforced ({status})]\n\n"
                ));
            }
            crate::managed_process::SandboxExecutionOutcome::Enforced { .. }
            | crate::managed_process::SandboxExecutionOutcome::Disabled => {}
        }
        let mut output_truncated = managed.stdout.is_truncated() || managed.stderr.is_truncated();
        if !stdout.is_empty() {
            let bounded = super::output::truncate_output(
                &stdout,
                self.max_output_lines,
                self.max_output_bytes,
            );
            output_truncated |= bounded != stdout;
            result.push_str(&bounded);
        }
        if !stderr.is_empty() {
            if !result.is_empty() {
                result.push_str("\n--- stderr ---\n");
            }
            let bounded = super::output::truncate_output(
                &stderr,
                self.max_output_lines,
                self.max_output_bytes,
            );
            output_truncated |= bounded != stderr;
            result.push_str(&bounded);
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
        let metadata = RawShellResultMetadata {
            exit_code: managed.exit_status.code(),
            output_truncated,
            cleanup: if managed.cleanup.errors.is_empty() {
                "complete"
            } else {
                "errors"
            },
            helper: if matches!(
                managed.sandbox,
                crate::managed_process::SandboxExecutionOutcome::Enforced { .. }
            ) {
                "codegg-sandbox-helper"
            } else {
                "none"
            },
        };
        Ok((result, output, managed.sandbox, metadata))
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
        session_id: Option<String>,
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
                    session_id: session_id.clone(),
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
                    target: codegg_core::jobs::ExecutionTarget::default(),
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
        session_id: Option<String>,
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
            session_id: session_id.clone(),
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
            ExecutionTarget, IdempotencyClass, JobKind, JobPayload, JobPriority, JobSource, NewJob,
            RetryPolicy,
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
            target: ExecutionTarget::default(),
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
    /// When `audit_ctx` carries the M001 trusted context plus the M002
    /// shared emitter, the executor emits the single `git_operation`
    /// event for this transition (the shell layer stays silent for the
    /// Git route so both origins converge on one audit event).
    ///
    /// Errors are returned to the caller — `BashTool::execute` MUST NOT retry
    /// through raw shell after this method runs.
    pub(crate) async fn dispatch_to_git(
        &self,
        request: &crate::command_intent::plan::GitExecutionRequest,
        canonical_workdir: Option<&Path>,
        input_workdir: Option<&Path>,
        timeout: Duration,
        audit_ctx: Option<&crate::tool::backend::ToolExecutionContext>,
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
        let exec = match audit_ctx.and_then(|ctx| ctx.execution_authorization.clone()) {
            Some(authorization) => exec.with_execution_authorization(authorization),
            None => exec,
        };
        // M002: thread the daemon-built audit pair so this routed
        // mutation emits the single executor-owned `git_operation`
        // event. Absent in legacy/harness callers (silent).
        let exec = match audit_ctx.and_then(|ctx| ctx.live_audit_hook()) {
            Some((audit, emitter)) => exec
                .with_execution_audit(audit.clone())
                .with_audit_emitter(emitter.clone()),
            None => exec,
        };

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
        session_id: Option<String>,
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
                    session_id: session_id.clone(),
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
                    target: codegg_core::jobs::ExecutionTarget::default(),
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
        session_id: Option<String>,
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
        let argv = shell_argv(command, self.windows_shell)?;
        let submitted = submission
            .submit(
                None,
                codegg_core::jobs::NewJob {
                    workspace_id,
                    session_id: session_id.clone(),
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
                    target: codegg_core::jobs::ExecutionTarget::default(),
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
        audit_ctx: Option<&crate::tool::backend::ToolExecutionContext>,
    ) -> Result<DispatchOutcome, ToolError> {
        if !active_route_matches_authorization(audit_ctx) {
            return Err(ToolError::Permission(
                "active command route cannot prove the accepted constrained sandbox; route refused"
                    .into(),
            ));
        }
        let session_id = audit_ctx.and_then(|ctx| ctx.session_id.clone());
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
                    session_id.clone(),
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
                self.dispatch_to_python_script(
                    script,
                    mode_str,
                    canonical_workdir,
                    timeout,
                    session_id.clone(),
                )
                .await
            }
            CommandDispatchTarget::RouteToManagedProcess { command, cwd, .. } => {
                let kind = match _plan.intent.kind {
                    CommandIntentKind::Build => codegg_core::jobs::JobKind::Build,
                    CommandIntentKind::Lint => codegg_core::jobs::JobKind::Lint,
                    CommandIntentKind::Format => codegg_core::jobs::JobKind::Format,
                    _ => codegg_core::jobs::JobKind::ManagedProcess,
                };
                self.dispatch_to_managed_process(
                    command,
                    Some(cwd),
                    timeout,
                    kind,
                    session_id.clone(),
                )
                .await
            }
            CommandDispatchTarget::RouteToGit { request, .. } => {
                // Track U unified dispatch: route typed Git operations
                // through `GitMutationExecutor` so they share the same
                // env policy, snapshot/delta, projection, and RunStore
                // semantics as native-tool invocations. Managed/unknown
                // plumbing falls through to the managed-argv path inside
                // `dispatch_to_git` without snapshot/delta persistence.
                self.dispatch_to_git(
                    request,
                    canonical_workdir,
                    input_workdir,
                    timeout,
                    audit_ctx,
                )
                .await
            }
            CommandDispatchTarget::RouteToShell { command, .. } => {
                self.dispatch_to_shell(command, canonical_workdir, timeout, session_id)
                    .await
            }
            CommandDispatchTarget::Rejected { reason } => Err(ToolError::Execution(format!(
                "command rejected: {}",
                reason
            ))),
        }
    }

    /// M002 background context: host-resolved owner lineage for an explicit
    /// background request. Never synthesized from model input; missing
    /// session fails without spawning.
    pub(crate) fn background_ctx(
        audit_ctx: Option<&crate::tool::backend::ToolExecutionContext>,
    ) -> Result<BashBackgroundCtx, ToolError> {
        let session_id = audit_ctx
            .and_then(|c| c.session_id.clone())
            .filter(|s| !s.is_empty())
            .ok_or_else(|| {
                ToolError::Execution(
                    "background bash requires a real host-originated session context".into(),
                )
            })?;
        let turn_id = audit_ctx.and_then(|c| c.turn_id.clone());
        let invocation_key = audit_ctx
            .and_then(|c| c.invocation_key.clone())
            .unwrap_or_else(|| format!("bash-bg:{}", uuid::Uuid::new_v4()));
        let owner_principal = audit_ctx.and_then(|c| {
            c.origin_principal
                .clone()
                .or_else(|| c.principal_identity.clone())
        });
        Ok(BashBackgroundCtx {
            session_id,
            turn_id,
            invocation_key,
            disposition: crate::scheduler::job_notifications::JobCompletionDisposition::NotifyOnly,
            owner_principal,
        })
    }

    /// Shared background submit: durable acceptance + durable subscription
    /// before the handle is returned. Crash between submit and subscribe is
    /// healed by retry with the same invocation key.
    async fn submit_bash_background(
        &self,
        kind: codegg_core::jobs::JobKind,
        kind_str: &str,
        payload: codegg_core::jobs::JobPayload,
        workspace_root: &Path,
        timeout: Duration,
        idempotency: codegg_core::jobs::IdempotencyClass,
        bg: &BashBackgroundCtx,
    ) -> Result<serde_json::Value, ToolError> {
        let Some(submission) = self.submission.clone() else {
            return Err(ToolError::Execution(
                "background bash requires the daemon scheduler".into(),
            ));
        };
        let Some(notifications) = self.job_notifications.clone() else {
            return Err(ToolError::Execution(
                "background bash requires the daemon job notification service".into(),
            ));
        };
        let workspace_id = submission
            .workspace_id_for_root(workspace_root)
            .await
            .map_err(|e| ToolError::Execution(e.to_string()))?;
        let key_digest = crate::tool::tool_program_context::stable_digest(&bg.invocation_key);
        let submission_key =
            crate::scheduler::submission::SubmissionKey::new(format!("bash-bg:{key_digest}"))
                .map_err(|e| ToolError::Execution(format!("invalid submission key: {e}")))?;
        let submitted = submission
            .submit(
                Some(submission_key),
                codegg_core::jobs::NewJob {
                    workspace_id,
                    session_id: Some(bg.session_id.clone()),
                    turn_id: bg.turn_id.clone(),
                    kind,
                    source: codegg_core::jobs::JobSource::Interactive,
                    priority: codegg_core::jobs::JobPriority::Interactive,
                    payload,
                    resource_request: codegg_core::jobs::ResourceRequest::for_kind(kind),
                    timeout: Some(timeout),
                    retry_policy: codegg_core::jobs::RetryPolicy::no_retry(),
                    idempotency,
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
                    target: codegg_core::jobs::ExecutionTarget::default(),
                },
            )
            .await
            .map_err(|e| ToolError::Execution(e.to_string()))?;
        notifications
            .subscribe_with_options(
                &submitted.job_id,
                kind_str,
                &bg.session_id,
                bg.turn_id.as_deref(),
                &bg.invocation_key,
                bg.disposition,
                bg.owner_principal.as_deref(),
                Some(submitted.workspace_id.as_str()),
                None,
            )
            .await
            .map_err(|e| ToolError::Execution(format!("background subscribe failed: {e}")))?;
        let now = chrono::Utc::now().timestamp_millis();
        let handle = crate::scheduler::job_notifications::job_handle_for(
            &submitted.job_id,
            kind_str,
            "submitted",
            now,
            None,
        );
        Ok(serde_json::json!({
            "job_id": handle.job_id,
            "kind": handle.kind,
            "status": handle.status,
            "submitted_at": handle.submitted_at,
            "inspect_ref": handle.inspect_ref,
            "cancel_ref": handle.cancel_ref,
            "on_complete": bg.disposition.as_str(),
            "message": "background bash accepted; one terminal notification will arrive without polling",
        }))
    }

    /// M002 background shell translation: the same scheduler-backed
    /// `JobKind::Shell` route the foreground `dispatch_to_shell` uses, with
    /// identical argv construction and platform launcher semantics. Used for
    /// both `RouteToShell` and raw-shell background (adopted translation).
    pub(crate) async fn submit_shell_background(
        &self,
        command: &str,
        canonical_workdir: Option<&Path>,
        timeout: Duration,
        bg: &BashBackgroundCtx,
    ) -> Result<serde_json::Value, ToolError> {
        let cwd = canonical_workdir
            .map(Path::to_path_buf)
            .unwrap_or_else(|| PathBuf::from("."));
        let argv = super::process::shell_argv(command, self.windows_shell)?;
        let payload = codegg_core::jobs::JobPayload::Shell {
            command: command.to_string(),
            argv: Some(argv),
            cwd: Some(cwd.to_string_lossy().into_owned()),
        };
        let cwd_ref = canonical_workdir.unwrap_or(Path::new("."));
        // Re-resolve the workspace root the same way the foreground path
        // does (canonical workdir, else ".").
        let root = if cwd_ref.as_os_str().is_empty() {
            Path::new(".")
        } else {
            cwd_ref
        };
        self.submit_bash_background(
            codegg_core::jobs::JobKind::Shell,
            "shell",
            payload,
            root,
            timeout,
            codegg_core::jobs::IdempotencyClass::NonIdempotent,
            bg,
        )
        .await
    }

    /// M002 background dispatch for active-routing targets. Scheduler-backed
    /// routes submit with trusted owner lineage; direct-only routes fail
    /// closed without spawning (no fallback, no reparse, no downgrade).
    pub(crate) async fn dispatch_command_target_background(
        &self,
        decision: &CommandDispatchTarget,
        plan: &CommandPlan,
        canonical_workdir: Option<&Path>,
        timeout: Duration,
        bg: &BashBackgroundCtx,
    ) -> Result<serde_json::Value, ToolError> {
        match decision {
            CommandDispatchTarget::RouteToTestRunner {
                argv,
                validated_command,
                ..
            } => {
                let _ = validated_command;
                let cwd = canonical_workdir.unwrap_or(Path::new("."));
                let payload = codegg_core::jobs::JobPayload::Test {
                    command: crate::command_intent::plan::NativeCommand::from_argv(argv.clone())
                        .map(|c| c.display())
                        .unwrap_or_default(),
                    argv: argv.clone(),
                    cwd: Some(cwd.to_string_lossy().into_owned()),
                    scope: Some("bash-dispatch".into()),
                    parent_run_id: None,
                };
                self.submit_bash_background(
                    codegg_core::jobs::JobKind::Test,
                    "test",
                    payload,
                    cwd,
                    timeout,
                    codegg_core::jobs::IdempotencyClass::SafeRepeat,
                    bg,
                )
                .await
            }
            CommandDispatchTarget::RouteToManagedProcess { command, cwd, .. } => {
                let kind = match plan.intent.kind {
                    CommandIntentKind::Build => codegg_core::jobs::JobKind::Build,
                    CommandIntentKind::Lint => codegg_core::jobs::JobKind::Lint,
                    CommandIntentKind::Format => codegg_core::jobs::JobKind::Format,
                    _ => codegg_core::jobs::JobKind::ManagedProcess,
                };
                let kind_str = kind.as_str().to_string();
                let cwd_owned = Path::new(cwd).to_path_buf();
                let payload = codegg_core::jobs::JobPayload::ManagedArgv {
                    argv: command.full_argv(),
                    cwd: Some(cwd_owned.to_string_lossy().into_owned()),
                };
                let cwd_path = cwd_owned.clone();
                self.submit_bash_background(
                    kind,
                    &kind_str,
                    payload,
                    &cwd_path,
                    timeout,
                    codegg_core::jobs::IdempotencyClass::SafeRepeat,
                    bg,
                )
                .await
            }
            CommandDispatchTarget::RouteToShell { command, .. } => {
                self.submit_shell_background(command, canonical_workdir, timeout, bg)
                    .await
            }
            CommandDispatchTarget::RouteToPythonScripting { script, mode, .. } => {
                let idempotency = match mode {
                    crate::command_intent::plan::PythonModeGuess::Transform => {
                        codegg_core::jobs::IdempotencyClass::NonIdempotent
                    }
                    _ => codegg_core::jobs::IdempotencyClass::SafeRepeat,
                };
                let mode_str = match mode {
                    crate::command_intent::plan::PythonModeGuess::Analyze => "analyze",
                    crate::command_intent::plan::PythonModeGuess::Transform => "transform",
                    crate::command_intent::plan::PythonModeGuess::Verify => "verify",
                    crate::command_intent::plan::PythonModeGuess::Unknown => "analyze",
                };
                // Background python uses the invocation-keyed submission key
                // (never source-hash-only) so two independent invocations of
                // the same script stay distinct while retries dedup.
                let cwd = canonical_workdir.unwrap_or(Path::new("."));
                let payload = codegg_core::jobs::JobPayload::Python {
                    script_path: String::new(),
                    args: vec![],
                    mode: mode_str.to_string(),
                    source: Some(script.clone()),
                    source_hash: Some(
                        crate::python_script::source_store::compute_digest(script),
                    ),
                    cwd: Some(cwd.to_string_lossy().into_owned()),
                    timeout_secs: Some(timeout.as_secs()),
                };
                self.submit_bash_background(
                    codegg_core::jobs::JobKind::Python,
                    "python",
                    payload,
                    cwd,
                    timeout,
                    idempotency,
                    bg,
                )
                .await
            }
            CommandDispatchTarget::RouteToNativeTool { command, .. } => Err(ToolError::Execution(
                format!(
                    "background execution_mode is not supported for native-tool route '{}': no safe scheduler-backed translation exists without changing command semantics; use foreground or a scheduler-backed shell/build/test form",
                    command.executable
                ),
            )),
            CommandDispatchTarget::RouteToGit { .. } => Err(ToolError::Execution(
                "background execution_mode is not supported for the git route: no safe scheduler-backed translation exists here; use foreground or the typed git tool"
                    .into(),
            )),
            CommandDispatchTarget::Rejected { reason } => Err(ToolError::Execution(format!(
                "command rejected: {reason}"
            ))),
        }
    }
}

/// Trusted owner lineage for one explicit background bash request.
pub(crate) struct BashBackgroundCtx {
    pub session_id: String,
    pub turn_id: Option<String>,
    pub invocation_key: String,
    pub disposition: crate::scheduler::job_notifications::JobCompletionDisposition,
    pub owner_principal: Option<String>,
}

fn active_route_matches_authorization(
    audit_ctx: Option<&crate::tool::backend::ToolExecutionContext>,
) -> bool {
    audit_ctx
        .and_then(|ctx| ctx.execution_authorization.as_ref())
        .is_none_or(|authorization| authorization.snapshot.sandbox_profile().is_full_host())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn active_routes_require_an_explicit_full_host_snapshot() {
        let mut context = crate::tool::backend::ToolExecutionContext::with_backend(
            crate::tool::backend::ToolBackendKind::Native,
        );
        context.execution_authorization =
            Some(crate::tool::authorization::ExecutionAuthorizationContext {
                snapshot: codegg_core::approval::ExecutionPolicySnapshot::capture(
                    codegg_core::approval::ApprovalMode::Yolo,
                    codegg_core::approval::SandboxProfile::WorkspaceWrite,
                    None,
                    None,
                    None,
                    None,
                    None,
                ),
                receipt: crate::permission::PermissionDecisionReceipt::allowed("yolo", None),
                command_preflight: None,
                scratch_scope: None,
            });
        assert!(!active_route_matches_authorization(Some(&context)));

        let mut full_host = context.clone();
        if let Some(authorization) = full_host.execution_authorization.as_mut() {
            authorization.snapshot = codegg_core::approval::ExecutionPolicySnapshot::capture(
                codegg_core::approval::ApprovalMode::Yolo,
                codegg_core::approval::SandboxProfile::FullHost,
                None,
                None,
                None,
                None,
                None,
            );
        }
        assert!(active_route_matches_authorization(Some(&full_host)));
        assert!(active_route_matches_authorization(None));
    }

    #[test]
    fn environment_policy_preserves_expected_vars() {
        let policy = bash_environment_policy();
        let debug = format!("{policy:?}");
        assert!(debug.contains("PATH"));
        assert!(debug.contains("USERPROFILE"));
        assert!(debug.contains("LOCALAPPDATA"));
    }

    #[test]
    fn raw_shell_keeps_command_text_in_one_argument() {
        let command = r#"echo "space & unicode λ"; $x = 'literal'"#;
        let argv = shell_argv(command, None).expect("shell argv");
        assert!(argv.len() >= 3);
        assert_eq!(argv.last().map(String::as_str), Some(command));
        #[cfg(unix)]
        assert_eq!(&argv[..2], &["sh", "-c"]);
        #[cfg(windows)]
        assert!(Path::new(&argv[0]).is_absolute());
    }

    #[cfg(windows)]
    #[tokio::test(flavor = "current_thread")]
    async fn native_shell_receives_a_private_per_run_temp_directory() {
        let tool = BashTool::new();
        let command = r#"Write-Output "$env:TEMP|$env:TMP""#;
        let (result, _) = tool
            .execute_via_raw_shell(command, Some(Path::new(".")), Duration::from_secs(10))
            .await
            .expect("Windows PowerShell launch");
        let line = result.lines().next().expect("temp output").trim();
        let (temp, tmp) = line.split_once('|').expect("both temp variables");
        assert_eq!(temp, tmp);
        assert!(temp.to_ascii_lowercase().contains("codegg-shell-"));
        assert!(
            !Path::new(temp).exists(),
            "per-run temp directory is cleaned"
        );
    }

    #[cfg(windows)]
    #[tokio::test(flavor = "current_thread")]
    async fn native_shell_can_run_common_toolchain_smoke_commands() {
        let tool = BashTool::new();
        let command = "rustc --version; cargo --version; git --version; python --version";
        let (result, _) = tool
            .execute_via_raw_shell(command, Some(Path::new(".")), Duration::from_secs(20))
            .await
            .expect("toolchain smoke command");
        assert!(result.contains("rustc 1.89.0"), "{result}");
        assert!(result.contains("cargo 1.89.0"), "{result}");
        assert!(
            result.to_ascii_lowercase().contains("git version"),
            "{result}"
        );
        assert!(result.to_ascii_lowercase().contains("python"), "{result}");
    }

    #[cfg(windows)]
    #[tokio::test(flavor = "current_thread")]
    async fn configured_cmd_shell_executes_raw_text() {
        let tool =
            BashTool::new().with_windows_shell(Some(crate::config::schema::WindowsShellKind::Cmd));
        let (version, _) = tool
            .execute_via_raw_shell(
                "echo cmd-shell-ready & echo %COMSPEC% & ver",
                Some(Path::new(".")),
                Duration::from_secs(10),
            )
            .await
            .expect("cmd version invocation");
        assert!(version.contains("cmd-shell-ready"), "{version}");
        assert!(
            version.to_ascii_lowercase().contains("cmd.exe"),
            "{version}"
        );
        assert!(
            version.to_ascii_lowercase().contains("windows"),
            "{version}"
        );
        let (result, _) = tool
            .execute_via_raw_shell(
                r#"echo "space & unicode λ""#,
                Some(Path::new(".")),
                Duration::from_secs(10),
            )
            .await
            .expect("cmd quoted argument invocation");
        assert!(result.contains("space"), "{result}");
        assert!(result.contains('λ'), "{result}");
    }

    #[cfg(windows)]
    #[tokio::test(flavor = "current_thread")]
    async fn configured_git_bash_executes_raw_text() {
        let tool = BashTool::new()
            .with_windows_shell(Some(crate::config::schema::WindowsShellKind::GitBash));
        let (result, _) = tool
            .execute_via_raw_shell(
                "printf '%s' 'git-bash-ready'",
                Some(Path::new(".")),
                Duration::from_secs(10),
            )
            .await
            .expect("Git Bash invocation");
        assert!(result.contains("git-bash-ready"), "{result}");
    }

    #[cfg(windows)]
    #[test]
    fn configured_windows_shells_resolve_outside_path() {
        let system_root = PathBuf::from(
            std::env::var_os("SystemRoot")
                .or_else(|| std::env::var_os("WINDIR"))
                .expect("Windows system root"),
        );
        for shell in [
            crate::config::schema::WindowsShellKind::PowerShell,
            crate::config::schema::WindowsShellKind::Cmd,
            crate::config::schema::WindowsShellKind::GitBash,
        ] {
            let argv = shell_argv("echo test", Some(shell)).expect("system shell");
            assert!(Path::new(&argv[0]).is_absolute());
            if matches!(shell, crate::config::schema::WindowsShellKind::GitBash) {
                let git_bash_candidates = ["ProgramFiles", "ProgramFiles(x86)"]
                    .into_iter()
                    .filter_map(std::env::var_os)
                    .filter(|value| !value.is_empty())
                    .map(PathBuf::from)
                    .map(|root| root.join("Git/bin/bash.exe"));
                assert!(
                    git_bash_candidates
                        .into_iter()
                        .any(|candidate| Path::new(&argv[0]) == candidate),
                    "unexpected Git Bash path: {}",
                    argv[0]
                );
            } else if matches!(shell, crate::config::schema::WindowsShellKind::PowerShell) {
                assert_eq!(
                    Path::new(&argv[0]),
                    system_root.join("System32/WindowsPowerShell/v1.0/powershell.exe")
                );
            } else {
                assert_eq!(
                    Path::new(&argv[0]),
                    system_root.join("System32").join("cmd.exe")
                );
                assert_eq!(&argv[1..4], &["/D", "/S", "/C"]);
            }
        }
        match shell_argv(
            "echo test",
            Some(crate::config::schema::WindowsShellKind::Wsl),
        ) {
            Ok(argv) => {
                assert!(Path::new(&argv[0]).is_absolute());
                assert_eq!(Path::new(&argv[0]), system_root.join("System32/wsl.exe"));
                assert_eq!(&argv[1..4], &["--exec", "sh", "-c"]);
            }
            Err(error) => assert!(
                error
                    .to_string()
                    .contains("configured Windows shell is unavailable"),
                "WSL absence should be reported explicitly: {error}"
            ),
        }
    }

    #[cfg(windows)]
    #[test]
    fn cmd_shell_text_moves_to_the_unescaped_command_channel() {
        let command = r#"echo "space & unicode λ""#;
        let mut argv = vec![
            r"C:\Windows\System32\cmd.exe".to_string(),
            "/D".to_string(),
            "/S".to_string(),
            "/C".to_string(),
            command.to_string(),
        ];
        let raw = extract_cmd_raw_command(command, &mut argv)
            .expect("valid cmd shell argv")
            .expect("cmd uses raw command channel");
        assert_eq!(argv, [r"C:\Windows\System32\cmd.exe", "/D", "/S", "/C"]);
        assert_eq!(raw.to_string_lossy(), format!("\"{command}\""));
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

#[cfg(test)]
mod timeout_partial_output_tests {
    use super::*;

    /// The regression: a timed-out command used to report only its command
    /// line, throwing away everything it had already printed. The model
    /// then had to re-run blind to discover why it went long.
    #[test]
    fn a_timeout_keeps_what_the_command_had_printed() {
        let msg = partial_timeout_report(
            "cargo build --release",
            "   Compiling codegg-core v0.1.0\nerror[E0433]: failed to resolve",
            "warning: unused variable",
        );
        assert!(msg.contains("cargo build --release"));
        assert!(
            msg.contains("failed to resolve"),
            "stdout must survive: {msg}"
        );
        assert!(
            msg.contains("unused variable"),
            "stderr must survive: {msg}"
        );
    }

    #[test]
    fn a_timeout_with_no_output_says_so_instead_of_looking_empty() {
        let msg = partial_timeout_report("sleep 600", "", "");
        assert!(msg.contains("no output was produced"), "{msg}");
        assert!(!msg.contains("stdout before termination"));
    }

    #[test]
    fn a_timeout_does_not_swamp_the_context_with_runaway_output() {
        let huge = "x".repeat(TIMEOUT_OUTPUT_BYTES * 3);
        let msg = partial_timeout_report("yes | head -c 999999", &huge, "");
        assert!(
            msg.len() < TIMEOUT_OUTPUT_BYTES * 2,
            "timeout text must stay bounded"
        );
        assert!(msg.contains("earlier output truncated"), "{msg}");
    }

    /// On a timeout the tail is the signal: it shows where the command
    /// actually stopped, which the head of a long build log does not.
    #[test]
    fn a_truncated_timeout_keeps_the_tail() {
        let mut data = "a".repeat(TIMEOUT_OUTPUT_BYTES);
        data.push_str("FINAL-LINE-MARKER");
        let (text, truncated) = tail_bounded(&data);
        assert!(truncated);
        assert!(
            text.contains("FINAL-LINE-MARKER"),
            "tail must be kept: {text}"
        );
    }

    #[test]
    fn short_output_is_returned_verbatim() {
        let (text, truncated) = tail_bounded("all good");
        assert_eq!(text, "all good");
        assert!(!truncated);
    }
}

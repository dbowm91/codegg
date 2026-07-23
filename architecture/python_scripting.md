# Python Scripting

First-class Python script execution with risk analysis, three execution modes, OS-level sandbox enforcement, and async subprocess management.

## Source

`src/python_script/` (8 files) — sole canonical module. The legacy `src/python_scripting.rs` has been removed.

## Module Structure

| File | Purpose |
|------|---------|
| `mod.rs` | Module root, re-exports, integration tests |
| `types.rs` | Core types: `PythonExecutionMode`, `PythonScriptSource`, `PythonCapabilityEnvelope`, `PythonRiskLevel`, `PythonRiskAssessment`, `PythonRiskScanner`, `PythonScriptRequest`, `PythonRunStatus`, `PythonRunResult`, `PythonCapabilityProfile`, `ExecutableRule`, `SandboxRequirement`, `SandboxBackend`, `CapabilityViolation`, `PythonPolicyDecision` |
| `analyze.rs` | AST-first risk analyzer with string-scanning fallback |
| `sandbox.rs` | `resolve_policy()`, `validate_subprocess_invocation()`, `derive_envelope()`, `check_compatibility()` |
| `snapshot.rs` | `WorkspaceSnapshot::capture(root)` and `diff()` for change detection |
| `executor.rs` | `execute_python_script(request)` — validates, runs risk analysis, enforces capabilities, captures snapshots, executes with timeout, generates diffs |
| `projection.rs` | `project_python_run(result)` — formats run results into model-facing markdown |
| `tool.rs` | `PythonScriptTool` — `Tool` trait impl for model-facing Python execution |

## Core Types

### `PythonExecutionMode`

| Mode | Description | Timeout |
|------|-------------|---------|
| `Analyze` | Read-only analysis | 60s |
| `Transform` | Mutating transformation | 60s |
| `Verify` | Test/verification | 300s |

### `PythonScriptRequest`

```rust
pub struct PythonScriptRequest {
    pub code: String,
    pub mode: PythonExecutionMode,
    pub cwd: PathBuf,
    pub timeout_secs: Option<u64>,
    pub session_id: Option<String>,
    pub intent: Option<String>,
    pub workspace_root: Option<PathBuf>,
}
```

`workspace_root` provides the authoritative workspace boundary for CWD containment checks. When set, CWD must be inside this root. Falls back to process cwd when `None`.

### `PythonRiskAssessment`

```rust
pub struct PythonRiskAssessment {
    pub level: PythonRiskLevel,
    pub reasons: Vec<String>,
    pub has_file_io: bool,
    pub has_file_read: bool,
    pub has_file_write: bool,
    pub has_subprocess: bool,
    pub has_network: bool,
    pub has_destructive_ops: bool,
    pub has_dynamic_execution: bool,
    pub imports: Vec<String>,
    pub scanner: PythonRiskScanner,
}
```

`PythonRiskScanner` enum: `Ast` | `Fallback` — indicates which analysis backend produced the result.

### `PythonRunResult`

```rust
pub struct PythonRunResult {
    pub status: PythonRunStatus,
    pub stdout: String,
    pub stderr: String,
    pub duration: Duration,
    pub mode: PythonExecutionMode,
    pub script_length: usize,
    pub risk: PythonRiskAssessment,
    pub capabilities: PythonCapabilityEnvelope,
    pub changed_files: Vec<PathBuf>,
    pub interpreter: String,
    pub diff: Option<String>,
    pub script_body_hash: Option<String>,
    pub stdout_label: Option<String>,
    pub stderr_label: Option<String>,
    pub diff_label: Option<String>,
}
```

Labels are pseudo-local run identifiers, not registered in any artifact store. They are not expandable via `context_read` or other tools.

`PythonRunResult` also carries enforcement evidence fields (all `#[serde(default)]`):
```rust
pub policy_decision: Option<PythonPolicyDecision>,
pub denied_capabilities: Vec<String>,
pub os_filesystem_isolation: bool,
pub os_network_isolation: bool,
pub effective_read_roots: Vec<PathBuf>,
pub effective_write_roots: Vec<PathBuf>,
pub allowed_subprocesses: Vec<ExecutableRule>,
pub enforcement_warnings: Vec<String>,
```

### `PythonCapabilityProfile`

Determines allowed filesystem roots, subprocess rules, and sandbox requirements per execution mode and risk level:

```rust
pub struct PythonCapabilityProfile {
    pub mode: PythonExecutionMode,
    pub read_roots: Vec<PathBuf>,
    pub write_roots: Vec<PathBuf>,
    pub allow_subprocess: bool,
    pub allowed_subprocesses: Vec<ExecutableRule>,
    pub allow_network: bool,
    pub allow_env: bool,
    pub allow_dependency_install: bool,
    pub allow_destructive_fs: bool,
    pub sandbox_requirement: SandboxRequirement,
}
```

Constructors: `analyze()`, `transform()`, `verify()`, `from_mode_risk_and_context()`.

### `ExecutableRule`

Controls which subprocess binaries are allowed in Verify mode:

```rust
pub struct ExecutableRule {
    pub command: String,
    pub arg_prefixes: Vec<String>,
    pub reason: String,
}
```

Default rules for Verify mode: `python3 -m pytest`, `python3 -m unittest`, `cargo test`, `cargo build`, `cargo check`.

### `SandboxBackend`

`Landlock` (Linux only, OS-level filesystem isolation), `PortableFallback` (env_clear + cwd containment + snapshot detection), or `None`.

### `PythonPolicyDecision`

```rust
pub struct PythonPolicyDecision {
    pub profile: PythonCapabilityProfile,
    pub denied: Vec<CapabilityViolation>,
    pub warnings: Vec<String>,
    pub enforcement_backend: SandboxBackend,
    pub os_filesystem_isolation: bool,
    pub os_network_isolation: bool,
}
```

## Risk Analysis

```rust
pub fn analyze_python_risk(code: &str) -> PythonRiskAssessment
```

**AST-first analysis**: Spawns `python3 -I` with the script piped via stdin to an inline AST scanner. The scanner walks the Python AST tree to extract imports, function calls, and risk indicators. It builds alias maps to resolve `import subprocess as sp; sp.run(...)` and `from subprocess import run; run(...)` forms through their aliases. Falls back to string scanning if Python is unavailable or parsing fails.

Detection targets:
- **High**: destructive ops (`shutil.rmtree`, `os.remove`, `os.unlink`, `chmod`, etc.)
- **Medium**: subprocess calls, network access
- **Low**: file I/O, dynamic execution (`eval`/`exec`/`compile`), suspicious imports
- **Safe**: no risk indicators

Priority: destructive_ops > subprocess/network > file_io > safe.

## Capability Enforcement

### Policy Resolution

```rust
pub fn resolve_policy(
    mode: PythonExecutionMode,
    code: &str,
    workspace_root: Option<&Path>,
) -> PythonPolicyDecision
```

Full enforcement pipeline:
1. Run AST risk analysis (`analyze_python_risk`)
2. Build capability profile via `PythonCapabilityProfile::from_mode_risk_and_context()`
3. Cross-check risk against profile for violations
4. Resolve enforcement backend (Landlock on Linux, PortableFallback elsewhere)
5. Produce `PythonPolicyDecision` with denied capabilities, warnings, and backend info

### Legacy Enforcement (backward compat)

```rust
pub fn check_compatibility(mode: PythonExecutionMode, code: &str) -> Vec<String>
pub fn derive_envelope(mode: PythonExecutionMode, code: &str) -> (PythonCapabilityEnvelope, PythonRiskAssessment)
```

The executor runs both `resolve_policy()` (new) and `derive_envelope()` (legacy) for backward compatibility.

Default envelopes per mode:
- `Analyze()`: read_workspace only
- `Transform()`: read + write workspace
- `Verify()`: read workspace + subprocess

`from_mode_and_risk(mode, risk)` denies capabilities flagged by risk analysis. Capability checks distinguish file reads from file writes:
- `has_file_read` with `read_workspace`
- `has_file_write` with `write_workspace`
- destructive ops with `destructive_fs`

Analyze mode allows workspace reads but denies writes. Transform mode allows non-destructive workspace writes. Verify mode allows subprocess but denies writes.

## Execution Pipeline

```rust
pub async fn execute_python_script(request: &PythonScriptRequest) -> PythonRunResult
```

Flow:
1. Compute script body SHA-256 hash for reproducibility tracking
2. Validate script length against `MAX_SCRIPT_LENGTH` (500KB)
3. Validate CWD (must exist, must be directory, must be inside workspace root when provided)
4. **Policy resolution**: `resolve_policy()` determines capability profile, denied capabilities, and enforcement backend
5. **Pre-execution capability check**: blocks scripts with denied capabilities before any child process is spawned
6. Legacy `derive_envelope()` + `check_compatibility()` run for backward compat evidence
7. Materialize script to temp file (under `.codegg/python_runs/`)
8. **Pre-execution snapshot** for ALL modes (Analyze, Transform, Verify)
9. Capture pre-execution file contents for diff generation
10. Find python interpreter (`VIRTUAL_ENV` > `python3` > `python`)
11. Execute with timeout, **minimal environment isolation** (`.env_clear()` + selective restore), and **OS-level sandbox**:
    - **Linux with Landlock**: filesystem restrictions via `landlock_restrict_self()` syscall; allowed paths = workspace + tmp + Python prefix + /usr/lib; denied paths = /proc, /sys, /dev, root home, .ssh, .aws
    - **Portable fallback**: env_clear + cwd containment + snapshot-based post-hoc change detection
12. **Post-execution snapshot and diff** for ALL modes:
    - Analyze/Verify: any file change is a policy violation → run failed with exit code -2
    - Transform: file changes are allowed and reported; textual diff generated
13. Generate artifact handles (`python_run://<id>/stdout`, `stderr`, `diff`)
14. Return `PythonRunResult` with all fields populated including enforcement evidence

Constants: `DEFAULT_TIMEOUT_SECS = 60`, `MAX_SCRIPT_LENGTH = 500_000`.

## Transform Mode Diff Generation

Transform mode captures pre-execution file contents and generates a human-readable textual diff showing:
- Modified files: `--- a/<path>` / `+++ b/<path>` with truncated old/new content
- New files: `--- /dev/null` / `+++ b/<path>` with truncated content
- Deleted files: `--- a/<path>` / `+++ /dev/null` with truncated old content

Per-file content capped at 4000 chars.

## Integration

Python scripts are routed from the command intent pipeline:
1. `classify_command()` classifies python commands as `PythonAnalyze|Transform|Verify`
2. `plan_execution()` maps to `ExecutionBackend::PythonScript`
3. `resolve_routing()` maps to `RoutingDecision::RouteToPythonScripting`

Registered in `src/tool/mod.rs` via `registry.register(PythonScriptTool)` in `with_options()`.

## Scheduler-Owned Execution (Milestone 001)

All production model-facing Python execution is now scheduler-owned. The scheduler is the sole admission authority; no production path executes Python directly outside scheduler authority.

### Source Input Contract

Before job creation, the script source is validated and its SHA-256 digest is computed. The job payload carries:
- `source: Option<String>` — inline source body (for scripts under 200KB)
- `source_hash: Option<String>` — SHA-256 hex digest, required when source is present
- `mode`, `cwd`, `timeout_secs` — execution parameters

Content-addressed source persistence is available via `PythonSourceStore` (`src/python_script/source_store.rs`) for restart recovery. Source is stored at `<workspace>/.codegg/python_sources/<sha256>.py` with atomic writes.

### PythonJobExecutor

`PythonJobExecutor` implements `JobExecutor` for `JobKind::Python`:
- Validates source reference and digest before launch
- Begins a `RunKind::Python` RunStore record before execution
- Invokes `execute_python_script` with cancellation support via `tokio::select!`
- Maps process cancellation, timeout, sandbox denial, and spawn failure to distinct executor status classes
- Records heartbeat/progress at execution start and completion
- Persists stdout, stderr, diff, and enforcement evidence as RunStore artifacts
- Registered in `register_default_executors()` alongside Test, ManagedArgv, and Subagent executors

### Tool Migration

Both `PythonScriptTool` and `BashTool`'s active Python routing submit through `JobSubmissionService` when the scheduler is enabled:
- Deterministic submission keys derived from source hash ensure idempotency
- Tools wait via `scheduler.wait_for_completion()` for the execution to finish
- When the scheduler is disabled, tools fall back to direct execution (fail-closed behavior — returns typed error, not direct fallback)
- Transform mode uses `IdempotencyClass::NonIdempotent`; Analyze/Verify use `SafeRepeat`

### Cancellation

Cancellation propagates through `CancellationToken` wired into the executor context:
- Pre-launch cancellation: job is cancelled before any process is spawned
- During execution: `tokio::select!` races the cancellation token against `execute_python_script`
- Post-cancellation: RunStore record is finalized with cancelled status, permits are released

### Recovery

Daemon-generation recovery marks interrupted Python attempts as `Interrupted`. Read-only modes (Analyze/Verify) may be requeued per `RecoveryPolicy`; Transform defaults to non-retryable.

## Canonical Delegation Entry Point

### DelegatedPythonRun

```rust
// src/python_script/tool.rs:16-19
pub struct DelegatedPythonRun {
    pub result: PythonRunResult,
    pub run_id: Option<RunId>,
}
```

The `run_id` is `Some` when the canonical Python subsystem successfully began a `RunKind::Python` record; `None` when no record could be begun or no `RunStore` was provided. This is the **record-ownership contract**: callers inspect `run_id` to determine whether to suppress duplicate persistence, while retaining the delegated result when it is absent.

```rust
impl DelegatedPythonRun {
    pub fn into_result(self) -> PythonRunResult { self.result }
    pub fn result(&self) -> &PythonRunResult { &self.result }
}
```

### execute_and_persist_python_script

```rust
// src/python_script/tool.rs:40-51
pub async fn execute_and_persist_python_script(
    request: &PythonScriptRequest,
    run_store: Option<&Arc<dyn RunStore>>,
) -> DelegatedPythonRun
```

Single entry point for canonical Python delegation. Both the model-facing `PythonScriptTool` and `BashTool`'s active routing dispatcher use this function. Calls `execute_python_script()` then `persist_python_run()`. Returns the run result plus an optional `RunId` proving the delegated record was begun.

### persist_python_run

```rust
// src/python_script/tool.rs:56-60
pub async fn persist_python_run(
    store: &Arc<dyn RunStore>,
    request: &PythonScriptRequest,
    result: &PythonRunResult,
) -> Option<RunId>
```

Returns the `RunId` if the run was successfully begun — callers use this as proof of delegated ownership.

### build_python_request

```rust
// src/python_script/tool.rs:269
fn build_python_request(input: &serde_json::Value) -> Result<PythonScriptRequest, ToolError>
```

Converts planner output (JSON) to a canonical `PythonScriptRequest`. Used by `PythonScriptTool::execute`.

### Called by BashTool

`BashTool::dispatch_to_python_script` at `src/tool/bash.rs:693-725` builds a `PythonScriptRequest` from the planner's validated script, mode, cwd, and workspace root, then calls `execute_and_persist_python_script` with the shared `RunStore`. This replaces the previous direct `python3 -c` invocation, ensuring policy resolution, sandbox enforcement, snapshots, and RunStore persistence all run through the canonical path.

```bash
cargo test -p codegg --lib tool::bash
```

## Tests

```bash
cargo test -p codegg --lib python_script
```

### Adversarial Testing

The Python sandbox has adversarial tests in `tests/python_sandbox_adversarial.rs` that validate escape and bypass resistance:

- **Alias bypass**: `import subprocess as sp; sp.run(...)` resolves through alias maps to detect subprocess calls
- **getattr bypass**: `getattr(__builtins__, '__import__')('subprocess')` style dynamic imports
- **shell=True bypass**: `subprocess.call(..., shell=True)` with command concatenation
- **pathlib escape**: `Path('..').resolve()` traversals that attempt to write outside workspace
- **Dynamic code execution**: `eval()`, `exec()`, `compile()` with embedded dangerous calls
- **Import chain resolution**: multi-level aliases (`from os import path; path.join(...)`)
- **sys.path manipulation**: modifying sys.path to import blocked modules

```bash
cargo test --test python_sandbox_adversarial
```

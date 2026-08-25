# Python Scripting

First-class Python script execution with AST-first risk analysis, three execution
modes, OS-level sandbox enforcement (Landlock on Linux), and managed-process
subprocess management.

## Purpose

Provide a sandboxed, policy-gated Python execution surface for the model-facing
`python_script` tool and BashTool's active Python routing. Scripts are statically
analyzed for risk, executed through the scheduler, and their outputs are projected
safely. The module is the domain authority for Python execution semantics.

## Where It Lives

`src/python_script/` (9 files)

| File | Purpose |
|------|---------|
| `mod.rs` | Module root, re-exports, integration tests |
| `types.rs` | Core types: modes, requests, risk, profiles, sandbox, policy |
| `analyze.rs` | AST-first risk analyzer with string-scanning fallback |
| `sandbox.rs` | `resolve_policy()`, `check_compatibility()`, `derive_envelope()` |
| `snapshot.rs` | `WorkspaceSnapshot::capture(root)` and `diff()` for change detection |
| `executor.rs` | `execute_python_script_with_cancellation()` — full pipeline |
| `projection.rs` | `project_python_run()`, `PythonProjector` impl |
| `tool.rs` | `PythonScriptTool`, `DelegatedPythonRun`, RunStore helpers |
| `source_store.rs` | Content-addressed source store at `.codegg/python_sources/` |

## How It Works

1. **Risk analysis**: `analyze_python_risk()` spawns `python3 -I` with the script
   piped via stdin to an inline AST scanner. The scanner walks the Python AST to
   extract imports, function calls, and risk indicators. It builds alias maps to
   resolve `import subprocess as sp; sp.run(...)` through their aliases. Falls back
   to string scanning if Python is unavailable or parsing fails.
2. **Policy resolution**: `resolve_policy()` runs risk analysis, builds a capability
   profile via `PythonCapabilityProfile::from_mode_risk_and_context()`, cross-checks
   risk against profile for violations, resolves the enforcement backend (Landlock on
   supported Linux, PortableFallback elsewhere), and produces a `PythonPolicyDecision`.
3. **Pre-execution check**: Denied capabilities block execution before any child
   process is spawned. Legacy `derive_envelope()` + `check_compatibility()` run for
   backward compatibility.
4. **Script materialization**: Script is written to a temp file under
   `.codegg/python_runs/` with a drop guard for cleanup.
5. **Snapshot**: Pre-execution file contents are captured for diff generation.
6. **Execution**: The script runs through `ManagedProcessService` with:
   - Wall-clock timeout (default 60s for Analyze/Transform, 300s for Verify)
   - Minimal environment isolation via `python_environment_policy()` (allows PATH,
     HOME, LANG, LC_ALL, VIRTUAL_ENV, PYTHONPATH, DYLD_LIBRARY_PATH)
   - Cancellation token support
   - On Linux with Landlock: the parent sends a bounded launch spec to
     `codegg-sandbox-helper`; the helper applies ABI-aware landlock rules, verifies
     `FullyEnforced` and `no_new_privs`, then `exec`s Python.
   - Portable fallback: env clearing + cwd containment + snapshot-based post-hoc
     change detection.
7. **Post-execution snapshot and diff**:
   - Analyze/Verify: any file change is a policy violation → exit code -2
   - Transform: file changes are allowed and reported; textual diff generated
8. **RunStore persistence**: `begin_python_run()` → `write_python_run_artifacts()` →
   `complete_python_run()` for the canonical RunStore lifecycle.

### Scheduler integration

All production model-facing Python execution flows through the scheduler:

- `PythonJobExecutor` (`src/scheduler/executors.rs:492`) implements `JobExecutor`
  for `JobKind::Python`. It validates source digest, begins a RunStore record,
  invokes `execute_python_script_with_cancellation()`, persists artifacts, and
  completes the record.
- `PythonScriptTool::execute()` submits through `JobSubmissionService` when the
  scheduler is enabled. When disabled, returns `ToolError::Disabled` (fail-closed).
- `BashTool::dispatch_to_python_script` calls `execute_and_persist_python_script()`
  (test-only helper; production path goes through the scheduler).

## Key Types & APIs

### PythonExecutionMode (`types.rs:259-267`)

| Mode | Description | Default Timeout | Subprocess | Writes |
|------|-------------|----------------|------------|--------|
| `Analyze` | Read-only analysis | 60s | Denied | Denied |
| `Transform` | Mutating transformation | 60s | Denied | Allowed (workspace) |
| `Verify` | Test/verification | 300s | Allowed (allowlisted) | Denied |

### PythonScriptRequest (`types.rs:474-484`)

```rust
pub struct PythonScriptRequest {
    pub code: String,
    pub mode: PythonExecutionMode,
    pub cwd: PathBuf,
    pub workspace_root: Option<PathBuf>,
    pub timeout_secs: Option<u64>,
    pub session_id: Option<String>,
    pub intent: Option<String>,
}
```

`workspace_root` provides the authoritative workspace boundary for CWD containment.
Falls back to process cwd when `None`.

### PythonRiskAssessment (`types.rs:437-449`)

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

`PythonRiskScanner`: `Ast | Fallback` — which analysis backend produced the result.

### PythonRiskLevel (`types.rs:418-424`)

`Safe | Low | Medium | High`

Priority: destructive_ops > subprocess/network > file_io/dynamic_exec/dep_install > safe.

### PythonCapabilityProfile (`types.rs:65-86`)

```rust
pub struct PythonCapabilityProfile {
    pub mode: PythonExecutionMode,
    pub read_roots: Vec<PathBuf>,
    pub write_roots: Vec<PathBuf>,
    pub allow_subprocess: bool,
    pub allowed_subprocesses: Vec<ExecutableRule>,
    pub allow_network: bool,
    pub allow_env: Vec<String>,
    pub allow_dependency_install: bool,
    pub allow_destructive_fs: bool,
    pub sandbox_requirement: SandboxRequirement,
}
```

Constructors: `analyze(workspace_root)`, `transform(workspace_root)`,
`verify(workspace_root)`, `from_mode_risk_and_context(mode, workspace_root, risk)`.
Risk analysis can only narrow capabilities, never widen.

### ExecutableRule (`types.rs:19-27`)

Controls which subprocess binaries are allowed in Verify mode:

```rust
pub struct ExecutableRule {
    pub command: String,
    pub arg_prefixes: Vec<String>,
    pub reason: String,
}
```

Default Verify rules: `cargo`, `cargo-test`, `pytest`, `python3 -m pytest`,
`go test`, `make test`, `make build`.

### PythonCapabilityEnvelope (`types.rs:310-421`)

Legacy capability envelope (backward compat). Fields: `read_workspace`,
`write_workspace`, `read_outside_workspace`, `write_outside_workspace`, `subprocess`,
`network`, `env_access`, `dependency_install`, `destructive_fs`.

### PythonPolicyDecision (`types.rs:239-256`)

```rust
pub struct PythonPolicyDecision {
    pub profile: PythonCapabilityProfile,
    pub denied: Vec<CapabilityViolation>,
    pub warnings: Vec<String>,
    pub enforcement_backend: SandboxBackend,
    pub os_filesystem_isolation: bool,
    pub os_network_isolation: bool,
    pub outcome: Option<SandboxOutcome>,
}
```

### SandboxBackend (`types.rs:182-190`)

`Landlock | PortableFallback | None`

### SandboxOutcome (`types.rs:204-219`)

```rust
pub enum SandboxOutcome {
    Enforced { backend: SandboxBackend, abi: u32 },
    Fallback { backend: SandboxBackend, reason: String },
    Disabled,
    Failed { kind: SandboxFailureKind, reason: String },
}
```

### PythonRunResult (`types.rs:519-566`)

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
    // Enforcement evidence (Phase 06)
    pub policy_decision: Option<PythonPolicyDecision>,
    pub denied_capabilities: Vec<String>,
    pub os_filesystem_isolation: bool,
    pub os_network_isolation: bool,
    pub effective_read_roots: Vec<PathBuf>,
    pub effective_write_roots: Vec<PathBuf>,
    pub allowed_subprocesses: Vec<ExecutableRule>,
    pub enforcement_warnings: Vec<String>,
}
```

Labels are pseudo-local run identifiers, not registered in any artifact store.

### DelegatedPythonRun (`tool.rs:16-28`)

```rust
pub struct DelegatedPythonRun {
    pub result: PythonRunResult,
    pub run_id: Option<RunId>,
}
```

The `run_id` is `Some` when the canonical Python subsystem successfully began a
`RunKind::Python` record; `None` when no record could be begun or no `RunStore`
was provided. Record-ownership contract: callers inspect `run_id` to determine
whether to suppress duplicate persistence.

### PythonSourceStore (`source_store.rs`)

Content-addressed store at `<workspace>/.codegg/python_sources/<sha256>.py`.
Persists, retrieves, and cleans up source files. Atomic writes via temp-file + rename.
Rejects symlinks, path traversal, oversized input (>2 MiB), and invalid UTF-8.

```rust
pub fn persist(&self, source: &str) -> Result<PythonSourceRef, PythonSourceError>
pub fn retrieve(&self, reference: &PythonSourceRef) -> Result<String, PythonSourceError>
pub fn remove(&self, reference: &PythonSourceRef)
pub fn cleanup_orphans(&self, active_digests: &[&str]) -> usize
```

`INLINE_SOURCE_MAX_BYTES = 200 KiB`, `SOURCE_STORE_MAX_BYTES = 2 MiB`.

### PythonProjector (`projection.rs:148-285`)

Implements `CommandOutputProjector` for the shell projection pipeline. Name: `"python"`.
Detects Python commands by argv prefix (`python3`, `python`, `pip`, `pip3`, `conda`).
Extracts Python diagnostic spans (tracebacks, error types) for artifact references.

### Free functions

```rust
// analyze.rs
pub fn analyze_python_risk(code: &str) -> PythonRiskAssessment

// sandbox.rs
pub fn resolve_policy(
    mode: PythonExecutionMode,
    code: &str,
    workspace_root: &Path,
) -> PythonPolicyDecision
pub fn check_compatibility(
    mode: PythonExecutionMode, code: &str,
) -> Vec<String>
pub fn derive_envelope(
    mode: PythonExecutionMode, code: &str,
) -> (PythonCapabilityEnvelope, PythonRiskAssessment)
pub fn validate_subprocess_invocation(
    profile: &PythonCapabilityProfile,
    cmd: &str,
    first_arg: Option<&str>,
) -> Result<(), String>

// executor.rs
pub async fn execute_python_script(
    request: &PythonScriptRequest,
) -> PythonRunResult
pub async fn execute_python_script_with_cancellation(
    request: &PythonScriptRequest,
    cancellation: CancellationToken,
) -> PythonRunResult

// projection.rs
pub fn project_python_run(result: &PythonRunResult) -> String
pub fn project_python_result(
    result: &PythonRunResult,
) -> ProjectionResult

// tool.rs
pub async fn begin_python_run(
    store: &Arc<dyn RunStore>,
    request: &PythonScriptRequest,
    result: &PythonRunResult,
) -> Option<RunHandle>
pub async fn persist_python_run(
    store: &Arc<dyn RunStore>,
    request: &PythonScriptRequest,
    result: &PythonRunResult,
) -> Option<RunId>

// source_store.rs
pub fn compute_digest(source: &str) -> String
```

## Configuration Surface

### Executor constants (`executor.rs:21-22`)

| Constant | Value | Purpose |
|----------|-------|---------|
| `DEFAULT_TIMEOUT_SECS` | 60 | Default for Analyze/Transform |
| `MAX_SCRIPT_LENGTH` | 500,000 | Script body size limit |

### Verify mode timeouts

The tool default for Verify mode is 300s (`tool.rs:525`). The scheduler may
override via `JobPayload.timeout_secs`.

### Source store constants (`source_store.rs:17-20`)

| Constant | Value | Purpose |
|----------|-------|---------|
| `INLINE_SOURCE_MAX_BYTES` | 204,800 | Max source inlineable in job payload |
| `SOURCE_STORE_MAX_BYTES` | 2,000,000 | Max total source accepted by store |

### Environment policy (`executor.rs:24-33`)

Inherited env vars: `PATH`, `HOME`, `LANG`, `LC_ALL`, `VIRTUAL_ENV`,
`PYTHONPATH`, `DYLD_LIBRARY_PATH`. All other env vars are cleared.

### Diff generation

Per-file content capped at 4000 chars (`executor.rs:669`).
File content capture limit: 2 MiB per file (`executor.rs:598`).

## Invariants & Gotchas

### Risk analysis is NOT safety

`analyze_python_risk()` is not a proof of safety. It feeds the capability
envelope and permission prompts. Runtime sandbox/snapshot checks remain required.
AST alias resolution handles `import subprocess as sp; sp.run(...)` and
`from subprocess import run; run(...)` forms.

### Capability narrowing only

Risk analysis can only deny capabilities, never widen them.
`PythonCapabilityProfile::from_mode_risk_and_context()` applies denials.
The legacy `PythonCapabilityEnvelope::from_mode_and_risk()` does the same.

### Sandbox failure is terminal

If the Landlock helper setup fails, the launch is aborted. Python code is NOT
started after a failed helper setup. `SandboxOutcome::Failed` is recorded.

### Post-execution snapshot enforcement

Analyze and Verify modes treat ANY file change as a policy violation (exit code -2).
Transform mode allows changes and reports them. Snapshot walks skip hidden dirs,
`target/`, `node_modules/`, `.codegg/`, and `Thumbs.db`.

### Scheduler-owned execution

`PythonScriptTool` and `BashTool` submit through `JobSubmissionService` when the
scheduler is enabled. When disabled, they return `ToolError::Disabled` — no direct
execution fallback exists (fail-closed).

### Idempotency

Analyze/Verify use `IdempotencyClass::SafeRepeat`. Transform uses
`IdempotencyClass::NonIdempotent`. Deterministic submission keys are derived from
source hash.

### Cancellation

Cancellation propagates through `CancellationToken` wired into the executor context.
Pre-launch cancellation is checked before process spawn. During execution,
`tokio::select!` races the cancellation token against `execute_python_script`.
Post-cancellation: RunStore record is finalized with cancelled status.

### Source integrity

The executor validates `source_hash` against SHA-256 of the inline source before
execution. Mismatches produce a `Failed` status with "source integrity check failed:
digest mismatch".

### Legacy payload rejection

`JobPayload::Python` entries with only `script_path` (no inline `source`) are
rejected with "inline source is required for scheduler-owned execution".

## Testing

Narrowest run:

```bash
cargo test -p codegg --lib python_script
```

Submodule targeting:

```bash
cargo test -p codegg --lib python_script::analyze      # 30+ tests
cargo test -p codegg --lib python_script::sandbox       # 30+ tests
cargo test -p codegg --lib python_script::executor      # 15+ tests
cargo test -p codegg --lib python_script::projection    # 17 tests
cargo test -p codegg --lib python_script::tool          # 2 tests
cargo test -p codegg --lib python_script::source_store  # 12 tests
cargo test -p codegg --lib python_script::snapshot      # 4 tests
cargo test -p codegg --lib python_script::tests         # module-level integration tests
```

### Adversarial testing

```bash
cargo test --test python_sandbox_adversarial
```

Validates escape and bypass resistance: alias bypass, getattr bypass,
shell=True bypass, pathlib escape, dynamic code execution, import chain
resolution, sys.path manipulation.

### BashTool integration

```bash
cargo test -p codegg --lib tool::bash
```

## Related Docs

- `architecture/command_intent.md` — how Python commands are classified and routed
- `architecture/scheduler.md` — scheduler admission and PythonJobExecutor lifecycle
- `architecture/human_shell.md` — projection pipeline
- `architecture/tool_programs.md` — tool program execution (separate subsystem)
- `architecture/security.md` — Landlock sandbox enforcement

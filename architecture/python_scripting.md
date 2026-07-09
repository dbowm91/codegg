# Python Scripting

First-class Python script execution with risk analysis, three execution modes, and async subprocess management.

## Source

`src/python_script/` (8 files) — sole canonical module. The legacy `src/python_scripting.rs` has been removed.

## Module Structure

| File | Purpose |
|------|---------|
| `mod.rs` | Module root, re-exports, integration tests |
| `types.rs` | Core types: `PythonExecutionMode`, `PythonScriptSource`, `PythonCapabilityEnvelope`, `PythonRiskLevel`, `PythonRiskAssessment`, `PythonRiskScanner`, `PythonScriptRequest`, `PythonRunStatus`, `PythonRunResult` |
| `analyze.rs` | AST-first risk analyzer with string-scanning fallback |
| `sandbox.rs` | `derive_envelope(mode, code)` and `check_compatibility(mode, code)` for capability enforcement |
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

```rust
pub fn check_compatibility(mode: PythonExecutionMode, code: &str) -> Vec<String>
pub fn derive_envelope(mode: PythonExecutionMode, code: &str) -> (PythonCapabilityEnvelope, PythonRiskAssessment)
```

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
4. Derive capability envelope and run risk analysis
5. **Pre-execution capability check**: `check_compatibility()` blocks scripts with denied capabilities BEFORE any child process is spawned
6. Materialize script to temp file (under `.codegg/python_runs/`)
7. **Pre-execution snapshot** for ALL modes (Analyze, Transform, Verify)
8. Capture pre-execution file contents for diff generation
9. Find python interpreter (`VIRTUAL_ENV` > `python3` > `python`)
10. Execute with timeout and **minimal environment isolation**:
    - Environment cleared via `.env_clear()` with selective restore of: `PATH`, `HOME`, `LANG`, `LC_ALL`, `VIRTUAL_ENV`, `PYTHONPATH`, `DYLD_LIBRARY_PATH`
11. **Post-execution snapshot and diff** for ALL modes:
    - Analyze/Verify: any file change is a policy violation → run failed with exit code -2
    - Transform: file changes are allowed and reported; textual diff generated
12. Generate artifact handles (`python_run://<id>/stdout`, `stderr`, `diff`)
13. Return `PythonRunResult` with all fields populated

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

## Tests

```bash
cargo test -p codegg --lib python_script
```

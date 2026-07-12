# Python Script Module

Module-based first-class Python scripting MVP with safety analysis, capability envelope, workspace snapshots, and model-facing tool.

## Source

`src/python_script/` (7 files)

## Module Structure

| File | Purpose |
|------|---------|
| `mod.rs` | Module root, re-exports, integration tests |
| `types.rs` | Core types: `PythonExecutionMode`, `PythonScriptSource`, `PythonCapabilityEnvelope`, `PythonRiskLevel`, `PythonRiskAssessment`, `PythonScriptRequest`, `PythonRunStatus`, `PythonRunResult` |
| `analyze.rs` | Enhanced static risk analyzer: imports, dynamic execution, file I/O, subprocess, network, destructive ops |
| `sandbox.rs` | `derive_envelope(mode, code)` and `check_compatibility(mode, code)` for capability enforcement |
| `snapshot.rs` | `WorkspaceSnapshot::capture(root)` and `diff()` for Transform mode change detection |
| `executor.rs` | `execute_python_script(request)` — validates, runs risk analysis, materializes to temp file, executes with timeout, captures snapshots |
| `projection.rs` | `project_python_run(result)` — formats run results into model-facing markdown |
| `tool.rs` | `PythonScriptTool` — `Tool` trait impl for model-facing Python execution |

## Core Types

### `PythonExecutionMode`

| Mode | Description | Timeout | Workspace |
|------|-------------|---------|-----------|
| `Analyze` | Read-only analysis | 60s | Read only |
| `Transform` | Mutating, captures diffs | 60s | Read + Write |
| `Verify` | Test/verification with subprocess | 300s | Read + Subprocess |

### `PythonCapabilityEnvelope`

9-field capability model: `read_workspace`, `write_workspace`, `read_outside_workspace`, `write_outside_workspace`, `subprocess`, `network`, `env_access`, `dependency_install`, `destructive_fs`.

Default envelopes per mode:
- `Analyze()`: read_workspace only
- `Transform()`: read + write workspace
- `Verify()`: read workspace + subprocess

`from_mode_and_risk(mode, risk)` denies capabilities flagged by risk analysis.

### `PythonRiskAssessment`

Static risk analysis with levels: `Safe` < `Low` < `Medium` < `High`.

Detection targets:
- **High**: destructive ops (`shutil.rmtree`, `os.remove`, `os.unlink`, `chmod`, etc.)
- **Medium**: subprocess calls, network access
- **Low**: file I/O, dynamic execution (`eval`/`exec`/`compile`), suspicious imports
- **Safe**: no risk indicators

### `WorkspaceSnapshot`

Captures file metadata (size + mtime) for workspace directories. `diff()` finds new, modified, and deleted files. Used by Transform mode to detect what changed during execution.

## Pipeline

```
Request → analyze risk → derive envelope → materialize to temp file
  → execute with timeout → capture post-snapshot (Transform)
  → diff snapshots → project result
```

## Integration

- Registered in `src/tool/mod.rs` via `registry.register(PythonScriptTool)` in `with_options()`
- Declared in `src/lib.rs` as `pub mod python_script`
- Command routing: `classify_command()` → `plan_execution()` → `resolve_routing()` → `RouteToPythonScripting`
- This is the sole canonical module — the legacy `src/python_scripting.rs` has been removed

## Canonical Delegation Entry Point

### DelegatedPythonRun

```rust
// src/python_script/tool.rs:16-19
pub struct DelegatedPythonRun {
    pub result: PythonRunResult,
    pub run_id: Option<RunId>,
}
```

Returned by `execute_and_persist_python_script`. The `run_id` is `Some` when the canonical subsystem persisted a `RunKind::Python` record; `None` when persistence failed. This is the **proof-of-persistence contract**.

### execute_and_persist_python_script

```rust
// src/python_script/tool.rs:40-51
pub async fn execute_and_persist_python_script(
    request: &PythonScriptRequest,
    run_store: Option<&Arc<dyn RunStore>>,
) -> DelegatedPythonRun
```

Single entry point for canonical Python delegation. Both `PythonScriptTool` and `BashTool::dispatch_to_python_script` use this function.

### Called by BashTool

`BashTool::dispatch_to_python_script` at `src/tool/bash.rs:693-725` uses `execute_and_persist_python_script` instead of direct `python3 -c`, ensuring policy resolution, sandbox enforcement, and RunStore persistence all run through the canonical path.

## Tests

105 tests covering type construction, serde roundtrips, risk analysis, sandbox compatibility, snapshot capture/diff, executor behavior, projection formatting, tool parameter parsing, and cross-module classify→plan→route integration.

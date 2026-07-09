# Python Scripting

First-class Python script execution with risk analysis, three execution modes, and async subprocess management.

## Source

`src/python_scripting.rs`

## Core Types

### `PythonScriptMode`

| Mode | Description | Timeout |
|------|-------------|---------|
| `Analyze` | Read-only analysis | 60s |
| `Transform` | Mutating transformation | 60s |
| `Verify` | Test/verification | 300s |

Methods: `label()` → `"analyze"`/`"transform"`/`"verify"`, `description()` → human-readable.

### `PythonScriptSource`

```rust
pub enum PythonScriptSource {
    Inline(String),
    FilePath(PathBuf),
}
```

### `PythonScript`

```rust
pub struct PythonScript {
    pub mode: PythonScriptMode,
    pub source: PythonScriptSource,
    pub timeout_secs: u64,
    pub cwd: Option<PathBuf>,
    pub env: Option<Vec<(String, String)>>,
}
```

Constructors: `analyze_inline(code)`, `transform_inline(code)`, `verify_inline(code)`, `from_file(path, mode)`.

### `PythonRiskAnalysis`

```rust
pub struct PythonRiskAnalysis {
    pub level: PythonRiskLevel,
    pub reasons: Vec<String>,
    pub has_file_io: bool,
    pub has_subprocess: bool,
    pub has_network: bool,
    pub has_destructive_ops: bool,
}
```

### `PythonRunResult`

```rust
pub struct PythonRunResult {
    pub status: PythonRunStatus,
    pub stdout: String,
    pub stderr: String,
    pub duration: Duration,
    pub mode: PythonScriptMode,
    pub script_length: usize,
}
```

## Risk Analysis

```rust
pub fn analyze_python_risk(code: &str) -> PythonRiskAnalysis
```

Static string-contains analysis:

| Pattern | Flag | Risk Level |
|---------|------|------------|
| `open(`, `write(`, `os.remove` | `has_file_io` | Low |
| `subprocess`, `os.system`, `os.popen` | `has_subprocess` | Medium |
| `requests.`, `urllib`, `http.client`, `socket.` | `has_network` | Medium |
| `shutil.rmtree`, `os.unlink`, `os.rmdir` | `has_destructive_ops` | High |

Priority: destructive_ops > subprocess/network > file_io > safe.

`requires_permission()` returns true for Medium or High risk.

## Script Execution

```rust
pub async fn run_python_script(script: &PythonScript) -> PythonRunResult
```

Flow:
1. Check script length against `MAX_SCRIPT_LENGTH` (500KB)
2. Find python command (`python3` on Unix, `python` on Windows)
3. Build args: `-c <code>` for Inline, `<path>` for FilePath
4. Set cwd and env if provided
5. `tokio::time::timeout` with `script.timeout_secs`
6. Return `PythonRunResult` with status, stdout, stderr, duration, mode, script_length

Constants: `DEFAULT_PYTHON_TIMEOUT_SECS = 60`, `MAX_SCRIPT_LENGTH = 500_000`.

## Integration

Python scripts are routed from the command intent pipeline:
1. `classify_command()` classifies python commands as `PythonAnalyze|Transform|Verify`
2. `plan_execution()` maps to `ExecutionBackend::PythonScript`
3. `resolve_routing()` maps to `RoutingDecision::RouteToPythonScripting`

Currently, Python routing is recognized and metadata is attached to BashTool output, but actual execution still goes through raw shell. Full Python script routing is planned for Phase 05.

## Tests

```bash
cargo test -p codegg --lib python_scripting
```

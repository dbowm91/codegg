# Command Routing

The command routing module resolves a `CommandPlan` into a concrete `RoutingDecision` that maps to a specific codegg subsystem. This is the third stage of the command intent pipeline (classify → plan → **route**).

## Source

`src/command_routing.rs`

## Core Types

### `RoutingDecision`

```rust
pub enum RoutingDecision {
    RouteToTestRunner {
        argv: Vec<String>,
        scope_label: String,
        validated_command: Option<String>,
    },
    RouteToShell {
        command: String,
        timeout_secs: Option<u64>,
    },
    RouteToPythonScripting {
        script: String,
        mode: PythonModeGuess,
        timeout_secs: Option<u64>,
    },
    RouteToNativeTool {
        tool_name: String,
        command: String,
    },
    RouteToManagedProcess {
        argv: Vec<String>,
        cwd: PathBuf,
        timeout_secs: Option<u64>,
    },
    Rejected {
        reason: String,
    },
}
```

## Routing Resolution

```rust
pub fn resolve_routing(plan: &CommandPlan) -> RoutingDecision
```

Maps `ExecutionBackend` → `RoutingDecision`:

| Backend | RoutingDecision |
|---------|----------------|
| `TestRunner { validated_command }` | `RouteToTestRunner { argv, scope_label: "command-intent:<label>", validated_command }` |
| `PythonScript { script, mode_guess }` | `RouteToPythonScripting { script, mode, timeout_secs }` |
| `NativeTool { tool_name }` | `RouteToNativeTool { tool_name, command }` |
| `GitMutating { tool_name, argv }` | `RouteToManagedProcess { argv, cwd, timeout_secs }` |
| `ManagedArgv { argv, cwd }` | `RouteToManagedProcess { argv, cwd, timeout_secs }` |
| `RawShell { command }` | `RouteToShell { command, timeout_secs }` |
| `Reject { reason }` | `Rejected { reason }` |

## Integration

`resolve_routing()` is called by:
- `BashTool::execute()` in `src/tool/bash.rs` — determines routing decision and dispatches

## Active Routing

Active routing is implemented and controlled by `CommandIntentMode::Active`. When active:

1. `BashTool::execute()` classifies the command, plans execution, validates via `validate_for_active_routing()`, and dispatches to the resolved subsystem
2. Dispatch methods: `dispatch_to_test_runner()`, `dispatch_to_native_tool()`, `dispatch_to_python_script()`, `dispatch_to_managed_process()`
3. On any dispatch failure, falls back to raw shell execution

### Kill Switches

- **Global**: `CODEGG_ROUTING_DISABLE=1` env var disables all active routing (falls back to observe)
- **Per-family**: `route_build`, `route_lint`, `route_format` set to `RouteLevel::Off` disables routing for that family
- Default mode is `Observe` — no active routing unless explicitly enabled

### Metrics

`RoutingMetric` is logged via `tracing::debug!` for every routing decision, including dispatch target and fallback reason.

### Safety

Active routing only fires when `validate_for_active_routing()` passes all 7 checks (SimpleArgv, High confidence, non-RawShell, non-Critical, no destructive/outside-workspace capabilities, no pending permissions). Commands that fail validation execute via raw shell as if in observe mode.

## Canonical Delegation Wiring

When active routing dispatches to TestRunner or Python, BashTool calls the canonical subsystem entry points — not raw shell. The result carries a `run_id` proving the delegated record was begun.

### DispatchOutcome (`src/tool/bash.rs:41-46`)

```rust
pub struct DispatchOutcome {
    pub result: String,
    pub output: std::process::Output,
    pub executor: ActualExecutor,
    pub delegated_run_id: Option<RunId>,
}
```

`delegated_run_id` is the canonical-record contract:
- `Some(run_id)` → delegated subsystem executed and owns a canonical RunStore record. BashTool skips duplicate persistence.
- `None` → the delegated subsystem executed without a canonical record (for example, no store was configured). BashTool retains that result, never re-runs the command, and uses caller persistence once when a store is available.

### TestRunner delegation flow

```
classify → plan → dispatch_to_test_runner (bash.rs:542)
  → TestScope::BashDispatch(argv) (types.rs:18)
  → resolve_and_run_test (resolve.rs:60-71)
      [bypasses allowlist re-validation — argv already validated by planner]
  → DelegatedTestRun { report, run_id } (runner.rs:258-260)
  → DispatchOutcome { ..., delegated_run_id }
  → caller suppresses persistence when run_id is Some
```

Key points:
- `TestScope::BashDispatch` (`src/test_runner/types.rs:18`) is a dedicated bypass variant: argv is consumed directly without the strict allowlist re-validation that `TestScope::CustomCommand` performs.
- `DelegatedTestRun` (`src/test_runner/runner.rs:258`) carries `report: TestReport` and `run_id: Option<RunId>`. Callers use `.into_report()` for display output.
- BashTool synthesizes a `std::process::Output`-shaped value from the report for code paths that inspect it (`src/tool/bash.rs:596-602`).

### Python delegation flow

```
classify → plan → dispatch_to_python_script (bash.rs:693)
  → PythonScriptRequest { code, mode, cwd, ... } (bash.rs:711-719)
  → execute_and_persist_python_script (python_script/tool.rs:40)
      [canonical entry point — applies AST risk scan, capability enforcement,
       Landlock sandboxing, snapshots, changed-file detection, diffs]
  → DelegatedPythonRun { result, run_id } (tool.rs:16-18)
  → DispatchOutcome { ..., delegated_run_id }
  → caller suppresses persistence when run_id is Some
```

Key points:
- `execute_and_persist_python_script` (`src/python_script/tool.rs:40`) is the single canonical entry point for both the model-facing `PythonScriptTool` and BashTool's active routing dispatcher.
- `DelegatedPythonRun` (`src/python_script/tool.rs:16`) carries `result: PythonRunResult` and `run_id: Option<RunId>`.
- `persist_python_run` (`src/python_script/tool.rs:56`) is best-effort; errors are logged, and `run_id` is `None` only when `run_store` is `None` or `begin_run` failed.
- Python projection uses `PythonProjector` via `project_python_result` for model-facing display.

### Raw-shell run-kind mapping (`src/command_outcome.rs:232-249`)

`run_kind_for_outcome()` maps `ActualExecutor::RawShell` → `RunKind::RawShell` **unconditionally**. Semantic intent (git, search, test, python) is never used to label raw-shell executions. Intent remains available through `planned_backend`, routing metadata, and intent kind fields.

### Persistence gating (`src/tool/bash.rs:1343-1354`)

```rust
let persist_run = match (ownership, delegated_run_id.as_ref()) {
    (DelegatedBackend, Some(_)) => false,        // subsystem owns persistence
    (DelegatedBackend, None) => self.run_store.is_some(), // caller persists once if possible
    _ => true,                                    // caller-owned
};
```

One logical execution is never retried merely because persistence is unavailable. A delegated backend without a `run_id` is treated as caller-owned only for the optional caller-side persistence attempt.

### Cross-references

- Validation evidence: `docs/validation/command-routing-execution-ownership.md`
- Execution ownership integration tests: `tests/command_routing_execution_ownership.rs`
- Adversarial routing tests: `tests/command_routing_adversarial.rs`

## Tests

```bash
cargo test -p codegg --lib command_routing
```

Includes 7 new tests for GitMutating routing, kill switch behavior, and fallback paths.

### Adversarial Test Coverage

```bash
cargo test --test command_routing_adversarial
```

139 adversarial tests covering: command smuggling, workspace escape, kill switches, Observe/Active modes, per-family RouteLevel overrides, validation failures, safe/dangerous git mutation routing, and full pipeline integration. These tests exercise the classify → plan → route pipeline end-to-end with adversarial inputs.

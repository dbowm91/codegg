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

## Tests

```bash
cargo test -p codegg --lib command_routing
```

Includes 7 new tests for GitMutating routing, kill switch behavior, and fallback paths.

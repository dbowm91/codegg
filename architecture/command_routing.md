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
| `ManagedArgv { argv, cwd }` | `RouteToManagedProcess { argv, cwd, timeout_secs }` |
| `RawShell { command }` | `RouteToShell { command, timeout_secs }` |
| `Reject { reason }` | `Rejected { reason }` |

## Integration

`resolve_routing()` is called by:
- `BashTool::execute()` in `src/tool/bash.rs` — determines routing metadata attached to output

The routing decision is currently informational for the bash tool — all commands still execute via raw shell. The metadata is attached to output for visibility and future structured routing. Python scripts are executed directly by the `PythonScriptTool` (model-facing tool), not via the bash tool routing path.

## Active Routing Status

**Active routing is intentionally deferred.** The classify → plan → route pipeline currently serves as an observe-only metadata annotation layer. All commands execute through raw shell regardless of the routing decision.

This deferral is deliberate until:
- Workspace root context is used for safety-critical path checks
- Python artifact handles are either real artifacts or explicitly documented as non-resolvable
- Command-intent risk metadata is not materially misleading

When `CommandIntentMode::Route` is configured, BashTool logs a warning and falls back to observe behavior. No config combination defaults to active routing.

## Tests

```bash
cargo test -p codegg --lib command_routing
```

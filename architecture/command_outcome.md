# Command Outcome

Execution outcome tracking and executor attribution.

## Purpose

`src/command_outcome.rs` records what backend actually executed a command versus what was planned, including fallback tracking. This provides auditability and attribution for the 3-stage command pipeline.

## Key Types

### ActualExecutor

Enum identifying which backend executed a command:

| Variant | Description |
|---------|-------------|
| `RawShell` | Direct shell execution |
| `ManagedArgv` | ManagedProcessService (scheduler-owned) |
| `NativeTool` | Tool registry (read, edit, grep, etc.) |
| `TestRunner` | Test execution backend |
| `PythonScript` | Python sandbox execution |
| `Git` | Git mutation/operation |
| `Rejected` | Command was rejected (permission/policy) |

Methods:
- `into_backend() -> PlannedBackend` — Convert to planned backend equivalent
- `label() -> &str` — Human-readable label

### ExecutionOutcome

Complete outcome record:

| Field | Type | Description |
|-------|------|-------------|
| `planned` | `PlannedBackend` | What the planner selected |
| `actual` | `ActualExecutor` | What actually ran |
| `fallback` | `bool` | Whether a fallback was used |
| `fallback_reason` | `Option<String>` | Why the fallback was triggered |

Methods:
- `identity() -> &str` — Executor identity string
- `with_fallback(reason) -> Self` — Mark as fallback with reason

## Usage in Pipeline

```
classify_command_with_context()     → CommandIntent
         │
plan_execution()                    → CommandPlan (with PlannedBackend)
         │
resolve_routing()                   → RoutingDecision
         │
Execute                             → ExecutionOutcome
         │
Record                              → ActualExecutor attribution
```

The `ExecutionOutcome` is attached to the command result for downstream consumers (TUI display, model context, audit logging).

## See Also

- [Command Intent](command_intent.md) — Stage 1: classification
- [Command Planner](command_planner.md) — Stage 2: backend selection
- [Command Routing](command_routing.md) — Stage 3: concrete dispatch
- [Tool](tool.md) — NativeTool executor

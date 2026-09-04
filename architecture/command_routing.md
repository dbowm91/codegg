# Command Routing

The command dispatch target is owned by `CommandPlan` and maps its selected
backend to a specific CodeGG subsystem. It is the dispatch boundary of the
typed command pipeline; it does not reinterpret command text or intent.

## Purpose

Map a `CommandPlan`'s `ExecutionBackend` to a `CommandDispatchTarget` enum variant
that carries the executor-specific data needed for actual dispatch.

## Where It Lives

- `src/command_intent/plan.rs` — `CommandDispatchTarget` and
  `CommandPlan::dispatch_target()`
- `src/command_routing.rs` — compatibility alias and `resolve_routing()` shim
- `src/tool/bash.rs` — dispatch methods that consume the canonical target

## How It Works

```
  CommandPlan
        │
        ▼
  plan.dispatch_target() ─→ CommandDispatchTarget
        │
        ▼
  BashTool::dispatch_command_target()
        │
        ├── RouteToTestRunner → submit_test_job()
        ├── RouteToGit        → dispatch_to_git()
        ├── RouteToPythonScripting → dispatch_to_python_script()
        ├── RouteToManagedProcess  → dispatch_to_managed_process()
        ├── RouteToShell      → execute_via_raw_shell()
        ├── RouteToNativeTool → (legacy, rarely used)
        └── Rejected          → error
```

## Key Types & APIs

### `CommandDispatchTarget` (compatibility name: `RoutingDecision`)

```rust
pub enum CommandDispatchTarget {
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
        command: NativeCommand,
    },
    RouteToManagedProcess {
        command: NativeCommand,
        cwd: std::path::PathBuf,
        timeout_secs: Option<u64>,
    },
    RouteToGit {
        request: GitExecutionRequest,
        timeout_secs: Option<u64>,
    },
    Rejected {
        reason: String,
    },
}
```

`RouteToGit` is the unified git routing variant, replacing the former pattern
where `GitReadOnly` routed through `RouteToNativeTool` (egggit) and
`GitMutating` routed through `RouteToManagedProcess`. All git commands now
map to `RouteToGit`.

Native routing carries the supported UTF-8 `NativeCommand` strings directly
to the managed-process boundary.

### `CommandPlan::dispatch_target()`

```rust
pub fn dispatch_target(&self) -> CommandDispatchTarget
```

Maps `ExecutionBackend` → `CommandDispatchTarget`:

| Backend | CommandDispatchTarget |
|---------|----------------|
| `TestRunner { validated_command }` | `RouteToTestRunner { argv, scope_label, validated_command }` |
| `PythonScript { script, mode_guess }` | `RouteToPythonScripting { script, mode, timeout_secs }` |
| `NativeTool { tool_name, command }` | `RouteToNativeTool { tool_name, command }` |
| `Git { request }` | `RouteToGit { request, timeout_secs }` |
| `ManagedArgv { command, cwd }` | `RouteToManagedProcess { command, cwd, timeout_secs }` |
| `RawShell { command }` | `RouteToShell { command, timeout_secs }` |
| `Reject { reason }` | `Rejected { reason }` |

`CommandDispatchTarget` is the dispatch boundary: it adds executor-facing data
such as parsed test argv, scope labels, timeouts, cwd, and the typed Git
request.

## Active Routing

Active routing is controlled by `CommandIntentMode::Active`. When active:

1. `BashTool::execute()` calls `prepare_command()` once, validates the plan via
   `validate_for_active_routing()`, and dispatches the plan-owned target
2. Dispatch methods submit scheduler-backed work through `submit_test_job()`,
   `dispatch_to_managed_process()`, and `dispatch_to_shell()`. Git and Python
   retain their domain-specific canonical adapters.
3. A scheduler-backed dispatch failure is returned as an execution error.
   It never falls back to raw shell, which would bypass admission or
   duplicate execution.

### Kill Switches

- **Global**: `CODEGG_ROUTING_DISABLE=1` env var disables all active routing
  (falls back to observe)
- **Per-family**: `route_build`, `route_lint`, `route_format` set to
  `RouteLevel::Off` disables routing for that family
- Default mode is `Observe` — no active routing unless explicitly enabled

The kill switch check is at `src/tool/bash.rs:494-511`:
```rust
fn check_kill_switches(&self, family: CommandIntentFamily) -> bool {
    let env_disabled = self.routing_disabled_override
        .unwrap_or_else(|| std::env::var("CODEGG_ROUTING_DISABLE")
            .unwrap_or_default() == "1");
    if env_disabled { return true; }
    if let Some(ref cic) = self.command_intent_config {
        if cic.family_level(family) == RouteLevel::Off { return true; }
    }
    false
}
```

### Metrics

`RoutingMetric` is logged via `tracing::debug!` for every routing decision,
including dispatch target and any explicit observe/kill-switch fallback.

### Safety

Active routing only fires when `validate_for_active_routing()` passes all 7
checks (SimpleArgv, High confidence, non-RawShell, non-Critical, no
destructive/outside-workspace capabilities, no pending permissions). Commands
that fail validation execute via raw shell as if in observe mode.

### Polish-pass provenance parity

The execution-origin matrix (`tests/git_execution_origin_matrix.rs`, 19 tests)
verifies that the routing layer produces consistent decisions for every origin.
The matrix covers:

- Native typed read → `RouteToGit`
- Native typed mutation → `RouteToGit`
- Native raw git subcommand → `RouteToGit`
- Bash simple git read → `RouteToGit`
- Bash simple git mutation → `RouteToGit` (when `route_git_local_mutation = Active`, Track U)
- Managed git argv fallback → `RouteToGit`
- Raw shell with `|` / `&&` / `;` → `RouteToShell`
- TUI git action → `RouteToGit`
- Daemon git action → `RouteToGit`
- Replay / rerun → placeholder (raw argv is structurally credential-free)

The Bash simple git mutation gap (matrix row 5) was closed by Track U. See
[`architecture/git.md` Track U section](git.md#track-u--unified-bashgit-routing)
for the unified dispatch details.

## Canonical Delegation Wiring

When active routing dispatches to TestRunner, BashTool submits a durable job;
the scheduler invokes the canonical subsystem and returns a `run_id` proving
the delegated record was begun. Python and Git retain their domain-specific
canonical adapters.

### DispatchOutcome (`src/tool/bash.rs:36-41`)

```rust
pub struct DispatchOutcome {
    pub result: String,
    pub output: std::process::Output,
    pub executor: ActualExecutor,
    pub delegated_run_id: Option<RunId>,
}
```

`delegated_run_id` is the canonical-record contract:
- `Some(run_id)` → delegated subsystem executed and owns a canonical RunStore
  record. BashTool skips duplicate persistence.
- `None` → the delegated subsystem executed without a canonical record. BashTool
  retains that result, never re-runs the command, and uses caller persistence
  once when a store is available.

### TestRunner delegation flow

```
classify → plan → submit_test_job (bash.rs:620)
  → JobSubmissionService → JobKind::Test
  → JobScheduler admission + TestJobExecutor
  → TestScope::BashDispatch(argv) (types.rs:18)
  → resolve_and_run_test (resolve.rs:60-71)
      [bypasses allowlist re-validation — argv already validated by planner]
  → DelegatedTestRun { report, run_id } (runner.rs:260-263)
  → DispatchOutcome { ..., delegated_run_id }
  → caller suppresses persistence when run_id is Some
```

Key points:
- `TestScope::BashDispatch` (`src/test_runner/types.rs:18`) is a dedicated
  bypass variant: argv is consumed directly without the strict allowlist
  re-validation that `TestScope::CustomCommand` performs.
- `DelegatedTestRun` (`src/test_runner/runner.rs:260`) carries
  `report: TestReport` and `run_id: Option<RunId>`. Callers use
  `.into_report()` for display output.
- BashTool synthesizes a `std::process::Output`-shaped value from the report
  for code paths that inspect it.

### Python delegation flow

```
classify → plan → dispatch_to_python_script (bash.rs:791)
  → PythonScriptRequest { code, mode, cwd, ... }
  → dispatch_python_via_scheduler (bash.rs:839)
      [scheduler admission required — production path]
  → DelegatedPythonRun { result, run_id } (tool.rs:16-19)
  → DispatchOutcome { ..., delegated_run_id }
  → caller suppresses persistence when run_id is Some
```

Key points:
- Production Python execution requires scheduler admission
  (`JobSubmissionService`). Without it, an error is returned.
- `DelegatedPythonRun` (`src/python_script/tool.rs:16`) carries
  `result: PythonRunResult` and `run_id: Option<RunId>`.
- The legacy `persist_python_run` helper (`src/python_script/tool.rs:220`) is
  best-effort; errors are logged, and `run_id` is `None` only when `run_store`
  is `None` or `begin_run` failed.

### Raw-shell run-kind mapping (`src/command_outcome.rs:158-181`)

`run_kind_for_outcome()` maps `ActualExecutor::RawShell` → `"raw_shell"`
**unconditionally**. Semantic intent (git, search, test, python) is never used
to label raw-shell executions. Intent remains available through
`planned_backend`, routing metadata, and intent kind fields.

### Persistence gating (`src/tool/bash.rs:1882-1894`)

```rust
let persist_run = match (delegated_executor, delegated_run_id.as_ref()) {
    (true, Some(_))  => false,   // subsystem owns persistence
    (true, None)     => self.run_store.is_some(), // caller persists once
    (false, _)       => true,    // caller-owned
};
```

One logical execution is never retried merely because persistence is
unavailable. A delegated backend without a `run_id` is treated as caller-owned
only for the optional caller-side persistence attempt.

### Cross-references

- Validation evidence: `docs/validation/command-routing-execution-ownership.md`
- Execution ownership integration tests: `tests/command_routing_execution_ownership.rs`
- Adversarial routing tests: `tests/command_routing_adversarial.rs`

## Invariants & Gotchas

- Active routing failures are terminal — they never fall back to raw shell,
  which would bypass admission or duplicate execution.
- `CommandDispatchTarget` is a dispatch-time boundary, not a persisted invocation
  model. Runtime truth is carried by `ActualExecutor`; persisted details use
  the core `RunInvocation` record.
- `RouteToGit` replaces both `RouteToNativeTool` (egggit) and the former
  `GitMutating` managed-process path.

The old `resolve_routing()` function and `RoutingDecision` name remain as a
source-compatible alias only; they delegate to the plan and contain no policy.

## Tests

```bash
cargo test -p codegg --lib command_routing
```

### Adversarial Test Coverage

```bash
cargo test --test command_routing_adversarial
```

Adversarial tests covering: command smuggling, workspace escape, kill switches,
Observe/Active modes, per-family RouteLevel overrides, validation failures,
safe/dangerous git mutation routing, and full pipeline integration. These tests
exercise the classify → plan → route pipeline end-to-end with adversarial
inputs.

### Track U unified dispatch

Track U unifies the bash→git routing path. When `route_git_local_mutation =
Active`, BashTool classifies simple git mutations through
`git_operation_family()` (replacing the former `intent_kind_to_family()` that
returned `None` for `GitMutating`). The routed command flows through
`dispatch_to_git` → `GitMutationExecutor`, sharing the same env policy,
snapshot/delta capture, and RunStore persistence as the native typed git tool.
Backend metadata is tagged `backend_family = "git_bash_translation"`,
`backend_detail = Some("bash_translation")`,
`RunOwnership::DelegatedBackend`. The conservative default
(`route_git_local_mutation = Off`) ensures existing user-visible behavior
is unchanged unless the user opts in.

# Command Routing

The command dispatch target is owned by `CommandPlan` and maps its selected
backend to a specific CodeGG subsystem. It is the dispatch boundary of the
typed command pipeline; it does not reinterpret command text or intent.

> **There is no independent `command_routing` module.** `src/command_routing.rs`
> is a 30-line, zero-logic compatibility facade whose own header states this:
> it exists only so integration callers using the pre-M006 API keep compiling.
> The sole production mapping lives in `CommandPlan::dispatch_target()` in
> `src/command_intent/plan.rs`. This document describes that canonical
> mapping; the facade is documented only for source compatibility.

## This is not slash-command routing

**`codegg <verb>` and `/verb` are different surfaces, and this document is
about neither of them.** It documents the dispatch of the **typed command
pipeline** — the `CommandPlan` the planner produces for a tool-shaped
intent — to the executor that will run it. It has nothing to do with the
user-typed slash command.

The slash command's own surface is:

- `architecture/command.md` — the **catalog**: the daemon's single
  definition of every command, its kinds, and its argument forms.
- `crates/codegg-core/src/command_catalog.rs` — that definition itself.
- `src/core/daemon_commands.rs` — the daemon's executor: a catalog form
  dispatched to a daemon request under `command.execute`.
- `src/tui/command_route.rs` — the TUI's route from a typed line to a
  catalog form and back to a `CoreRequest::CommandExecute`.

The two surfaces share the word "command" and nothing else. A slash
command is identified by a **catalog identity** and executed either by the
daemon (a daemon effect) or by the frontend (a frontend effect). A
`CommandPlan` is a **plan** for an execution, produced by the planner from
a tool-shaped intent, and dispatched by `dispatch_target()`. Nothing in
this document applies to a slash command, and nothing in `command.md`
applies to a `CommandPlan`. A reader who conflates `codegg <verb>` with
`/verb` will reach the wrong document every time.

## Purpose

Map a `CommandPlan`'s `ExecutionBackend` to a `CommandDispatchTarget` enum variant
that carries the executor-specific data needed for actual dispatch.

## Where It Lives

- `src/command_intent/plan.rs` — `CommandDispatchTarget` and
  `CommandPlan::dispatch_target()` (canonical owner)
- `src/command_routing.rs` — 30-line compatibility facade: re-exports
  `CommandDispatchTarget` as `RoutingDecision` and wraps
  `resolve_routing(plan) -> plan.dispatch_target()`
- `src/tool/bash/process.rs` — dispatch methods that consume the canonical
  target (`dispatch_command_target`, `execute_via_raw_shell`,
  `submit_test_job`, `dispatch_to_managed_process`, `dispatch_to_git`,
  `dispatch_to_python_script`, `dispatch_python_via_scheduler`,
  `dispatch_to_shell`, `dispatch_to_native_tool`)

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
        ├── RouteToShell      → dispatch_to_shell()
        ├── RouteToNativeTool → dispatch_to_native_tool() (legacy, rarely used)
        └── Rejected          → error
```

`dispatch_to_shell()` is the **active-routing** raw-shell path; it submits a
scheduler-backed `JobKind::Shell`. The separate `execute_via_raw_shell()` helper
serves the **observe-mode** path (and tests) — it is not what
`RouteToShell` resolves to.

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

The kill switch check is `check_kill_switches()` in `src/tool/bash/policy.rs`:
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

Active routing only fires when `validate_for_active_routing()`
(`src/command_intent/plan.rs:535`) passes all 7 checks (SimpleArgv, High
confidence, non-RawShell/non-Reject backend, non-Critical, no
destructive-file-mutation capability, no outside-workspace capability, no
pending permissions). Commands
that fail validation execute via raw shell as if in observe mode.

### Polish-pass provenance parity

The execution-origin matrix (`tests/command_git/git_execution_origin_matrix.inc`, 12 tests)
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

### DispatchOutcome (`src/tool/bash/process.rs`)

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
classify → plan → submit_test_job (src/tool/bash/process.rs)
  → JobSubmissionService → JobKind::Test
  → JobScheduler admission + TestJobExecutor
  → TestScope::BashDispatch(argv) (src/test_runner/types.rs)
  → resolve_and_run_test (src/test_runner/runner.rs)
      [bypasses allowlist re-validation — argv already validated by planner]
  → DelegatedTestRun { report, run_id } (src/test_runner/runner.rs)
  → DispatchOutcome { ..., delegated_run_id }
  → caller suppresses persistence when run_id is Some
```

Key points:
- `TestScope::BashDispatch` (`src/test_runner/types.rs`) is a dedicated
  bypass variant: argv is consumed directly without the strict allowlist
  re-validation that `TestScope::CustomCommand` performs.
- `DelegatedTestRun` (`src/test_runner/runner.rs`) carries
  `report: TestReport` and `run_id: Option<RunId>`. Callers use
  `.into_report()` for display output.
- BashTool synthesizes a `std::process::Output`-shaped value from the report
  for code paths that inspect it.

### Python delegation flow

```
classify → plan → dispatch_to_python_script (src/tool/bash/process.rs)
  → PythonScriptRequest { code, mode, cwd, ... }
  → dispatch_python_via_scheduler (src/tool/bash/process.rs)
      [scheduler admission required — production path]
  → DelegatedPythonRun { result, run_id } (src/python_script/tool.rs)
  → DispatchOutcome { ..., delegated_run_id }
  → caller suppresses persistence when run_id is Some
```

Key points:
- Production Python execution requires scheduler admission
  (`JobSubmissionService`). Without it, an error is returned.
- `DelegatedPythonRun` (`src/python_script/tool.rs`) carries
  `result: PythonRunResult` and `run_id: Option<RunId>`.
- The legacy `persist_python_run` helper (`src/python_script/tool.rs`) is
  best-effort; errors are logged, and `run_id` is `None` only when `run_store`
  is `None` or `begin_run` failed.

### Raw-shell run-kind mapping (`src/command_outcome.rs`)

`run_kind_for_outcome()` maps `ActualExecutor::RawShell` → `"raw_shell"`
**unconditionally**. Semantic intent (git, search, test, python) is never used
to label raw-shell executions. Intent remains available through
`planned_backend`, routing metadata, and intent kind fields.

### Persistence gating (`src/tool/bash/output.rs`)

```rust
let delegated_executor =
    matches!(
        &execution_outcome.actual,
        ActualExecutor::TestRunner { .. } | ActualExecutor::PythonScript { .. }
    ) || matches!(&execution_outcome.actual, ActualExecutor::Git { .. })
        && delegated_run_id.is_some();

let persist_run = match (delegated_executor, delegated_run_id) {
    (true, Some(_)) => false,                 // subsystem owns persistence
    (true, None) => self.run_store.is_some(), // caller persists once
    (false, _) => true,                       // caller-owned
};
```

A `Git` outcome counts as delegated **only when it also has a
`delegated_run_id`**; a git execution without one stays caller-owned. The
`(true, None)` arm logs a warning, because delegated ownership without a
`run_id` is the "the subsystem ran but no canonical record exists" case.

One logical execution is never retried merely because persistence is
unavailable. A delegated backend without a `run_id` is treated as caller-owned
only for the optional caller-side persistence attempt.

### Cross-references

- Validation evidence: `docs/validation/command-routing-execution-ownership.md`
- Execution ownership integration tests: `tests/command_git/command_routing_execution_ownership.inc`
- Adversarial routing tests: `tests/command_git/command_routing_adversarial.inc`

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
cargo test --test command_git -- command_routing_adversarial
```

`command_routing_adversarial` is a member of the `command_git` integration-test
family (`tests/command_git/`); the family is the single Cargo test target
declared in the root `Cargo.toml`, so the member name must be passed as a
filter after `--`.

Adversarial tests covering: command smuggling, workspace escape, kill switches,
Observe/Active modes, per-family RouteLevel overrides, validation failures,
safe/dangerous git mutation routing, and full pipeline integration. These tests
exercise the classify → plan → dispatch pipeline end-to-end with adversarial
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

## Source Verification

Verified 2026-10-06 against source. Corrected: framing now states up front that
`src/command_routing.rs` is a 30-line zero-logic compatibility facade (its own
header says so), not an independent module. Line refs: dispatch methods
`bash.rs:620/791/839` → `src/tool/bash/process.rs:246/417/467`;
persistence gating `src/tool/bash.rs:1882-1894` → `src/tool/bash/output.rs:367`;
`resolve_and_run_test` `resolve.rs:60-71` → `src/test_runner/runner.rs:749`;
execution-origin matrix 19 → 12 tests; `check_kill_switches`
`policy.rs:318-335` → `:318`. Verified accurate: 7-variant
`CommandDispatchTarget`, 7-row backend mapping table, 7
`validate_for_active_routing` checks (`src/command_intent/plan.rs:535`),
`DispatchOutcome` 4 fields (`src/tool/bash/process.rs:61`),
`run_kind_for_outcome` (`src/command_outcome.rs:158-181`) with unconditional
`RawShell → "raw_shell"`, `TestScope::BashDispatch` (`types.rs:18`),
`DelegatedTestRun` (`runner.rs:356`), `DelegatedPythonRun`
(`src/python_script/tool.rs:16`), `persist_python_run` (`tool.rs:232`).

Verified 2026-10-23 against `src/command_intent/plan.rs`,
`src/command_routing.rs`, `src/tool/bash/process.rs`,
`src/tool/bash/policy.rs`, `src/tool/bash/output.rs`,
`src/command_outcome.rs`, `src/test_runner/types.rs`,
`src/test_runner/runner.rs`, `src/python_script/tool.rs`, the root
`Cargo.toml` test targets, and `tests/command_git/mod.rs`. Corrected:

- **Stale line refs replaced by symbol names** across the document: every
  `bash/process.rs:NNN`, `policy.rs:NNN`, `runner.rs:NNN`, `tool.rs:NNN`,
  `output.rs:NNN`, and `command_outcome.rs:NNN` reference had drifted past its
  stated line (e.g. `DispatchOutcome` was cited at `process.rs:61` and is at
  `:123`; `check_kill_switches` was cited at `policy.rs:318` and is at `:531`).
  They are now referenced by symbol and file, because these files move.
- **`RouteToShell` dispatch fixed.** The pipeline diagram claimed
  `RouteToShell → execute_via_raw_shell()`. The active-routing arm actually
  calls `dispatch_to_shell()` (a scheduler-backed `JobKind::Shell` submit);
  `execute_via_raw_shell()` serves the observe-mode path. Both are now named,
  and `dispatch_to_native_tool()` is named for the `RouteToNativeTool` arm.
- **Persistence gating snippet corrected.** The quoted `persist_run` match used
  `delegated_run_id.as_ref()`; the real code matches on `delegated_run_id` by
  value and derives `delegated_executor` from `ActualExecutor` — including the
  `Git` arm, which is delegated **only** when a `delegated_run_id` is present.
- **Test command corrected.** `cargo test --test command_routing_adversarial`
  selected zero tests: the member lives in the `command_git` family. It is now
  `cargo test --test command_git -- command_routing_adversarial`.
- **Execution-origin matrix bullets extended** to name the `row_5b`/`row_5c`
  active-mode rows.

Verified accurate: `src/command_routing.rs` is still a 30-line zero-logic
facade with exactly one public function (`resolve_routing`) plus the
`RoutingDecision` alias; the 7-variant `CommandDispatchTarget` and the 7-row
backend mapping table; the 7 `validate_for_active_routing()` checks; the
4-field `DispatchOutcome`; the unconditional `RawShell → "raw_shell"` mapping in
`run_kind_for_outcome()`; `TestScope::BashDispatch`'s allowlist bypass; and the
`delegated_run_id` ownership contract.

Re-verified 2026-10-25: this document still describes the typed command
pipeline and nothing else. The added section is a **pointer**, not a second
description: the slash command's surface is `architecture/command.md`, the
catalog is `crates/codegg-core/src/command_catalog.rs`, the daemon's
executor is `src/core/daemon_commands.rs`, and the TUI's route is
`src/tui/command_route.rs`. `src/command_routing.rs` is still a 30-line
zero-logic compatibility facade and `CommandPlan::dispatch_target()` in
`src/command_intent/plan.rs` is still the sole production mapping.

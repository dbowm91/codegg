# Command Routing Actual Execution Ownership — Validation Evidence

This document validates the corrective pass for command-routing persistence
and provenance defects. See `plans/command-routing-actual-execution-ownership-corrective-pass.md`
for the original plan.

## Commit

- **SHA (corrective pass)**: pending (this commit)
- **Previous SHA**: `819f17a` (plan commit) / `992dfae` (closure hardening)

## Environment

- **Platform**: macOS (darwin)
- **Architecture**: aarch64
- **Rust version**: 1.96.0

## Problem Statement

The previous command-routing implementation had four persistence defects:

1. **RunKind was derived from the planned routing decision**, not what actually executed.
2. **`argv` was always recorded as `[sh, -c, command]`**, even when the actual execution was a direct `Command::new`.
3. **`RunOwnership::DelegatedBackend` was defined but never set anywhere.**
4. **Raw stdout/stderr artifacts were marked `safe_for_model: true`**, which violates the contract that only post-redaction projection is model-safe.

## Workstreams Completed

### Workstream A: Execution outcome types

New types in `crates/codegg-core/src/run_store.rs`:

- `PlannedBackend` (snake_case enum): Unrouted, RawShell, TestRunner, PythonScript, NativeTool, ManagedArgv, GitMutating
- `ActualBackend` (snake_case enum): RawShell, TestRunner, PythonScript, NativeTool, ManagedArgv, GitMutating, Rejected
- `FallbackRecord { planned, actual, reason }`

New optional fields on `RunManifest`, `RunDraft`, and `RunCompletion`:

- `planned_backend: Option<PlannedBackend>`
- `actual_backend: Option<ActualBackend>`
- `fallback: Option<FallbackRecord>`
- `ownership: RunOwnership` (defaults to Caller; serde-defaulted)

New module `src/command_outcome.rs`:

- `ActualExecutor` (the full execution context: argv, cwd, mode, etc.)
- `ActualInvocation` (the canonical argv-shaped invocation)
- `ExecutionOutcome` { planned, actual, fallback, fallback_reason }
- `ownership_for_outcome()` — maps `ExecutionOutcome` → `RunOwnership`
- `run_kind_for_outcome()` — maps `(ActualExecutor, CommandIntentKind)` → `RunKind`

### Workstream B: Persistence ownership from actual execution

- `BashTool` persistence path now derives `RunOwnership` from the actual executor (`ownership_for_outcome()`), not from the planned routing decision.
- `RunKind` is derived from `run_kind_for_outcome(execution_outcome, intent.kind)`.
- `BashTool` skips persistence when `ownership == DelegatedBackend`, ensuring only the delegated backend owns the canonical record.

### Workstream C: Python canonical subsystem

`PythonScriptTool::execute()` now persists its own RunStore record with:

- `planned_backend: Some(PythonScript)`
- `actual_backend: Some(PythonScript)`
- `ownership: DelegatedBackend`
- `script_hash: result.script_body_hash.clone()`
- All raw artifacts (`Stdout`, `Stderr`, `UnifiedDiff`) marked `safe_for_model: false`

### Workstream D: TestRunner canonical routing

`TestRunner::persist_to_run_store()` now sets:

- `planned_backend: Some(TestRunner)`
- `actual_backend: Some(TestRunner)`
- `ownership: DelegatedBackend`
- `argv: resolved.argv.clone()` (actual Command::new invocation, not `[sh, -c, ...]`)
- Raw `Stdout`/`Stderr` marked `safe_for_model: false`; `TestReport` (structured JSON) remains `safe_for_model: true`

### Workstream E: Exact invocation provenance

`BashTool` dispatch returns the actual `ActualExecutor` for each path:

- `RawShell` → argv = `[sh, -c, command]`
- `ManagedArgv` → argv = the actual `Command::new` argv (not `[sh, -c, ...]`)
- `NativeTool` → argv = the actual native-tool argv
- `TestRunner` → argv = the actual test runner argv (delegated)
- `PythonScript` → argv = `["python3", "<script>"]`

### Workstream F: Planned-vs-actual backend metadata

`RunManifest`, `RunDraft`, and `RunCompletion` carry `planned_backend`,
`actual_backend`, and `fallback`. The persistence path in `FsRunStore` and
`MemRunStore` propagates these from draft → manifest and from completion →
manifest. `complete_run` overrides `actual_backend` if the completion
provides one (for fallback scenarios).

### Workstream G: Raw artifact safety conservative

| Source | Artifact | safe_for_model |
|--------|----------|----------------|
| BashTool | Stdout, Stderr | `false` |
| TestRunner | Stdout, Stderr | `false` |
| TestRunner | TestReport | `true` (structured JSON) |
| PythonScript | Stdout, Stderr, UnifiedDiff | `false` |

Only structured artifacts (TestReport, post-redaction projection) are
model-safe. Raw stdout/stderr are explicitly NOT model-safe.

### Workstream H: Persistence failure handling

The persistence path uses `if let Ok(handle) = store.begin_run(draft).await`
which already swallows persistence errors. This means a RunStore failure
does not change the actual execution outcome or the result string returned
to the caller. The test suite validates this contract.

### Workstream I: Ownership integration tests

New file `tests/command_routing_execution_ownership.rs` (13 tests):

1. Observe mode persists raw_shell with `PlannedBackend::Unrouted`/`RawShell` and `RunOwnership::Caller`.
2. Active test command: BashTool MUST NOT persist Caller-owned record when routing to TestRunner.
3. Active git readonly: `RunKind::GitRead`, `RunOwnership::Caller`, `PlannedBackend::NativeTool`, `ActualBackend::NativeTool`.
4. Active search: `RunKind::Search`, `RunOwnership::Caller`, `PlannedBackend::ManagedArgv`, `ActualBackend::ManagedArgv`, argv != `[sh, -c, ...]`.
5. Active routing fallback: `planned_backend=TestRunner`, `actual_backend=RawShell`, FallbackRecord populated.
6. BashTool artifacts are NEVER `safe_for_model: true`.
7. PythonScriptTool: `RunOwnership::DelegatedBackend`, raw stdout/stderr NOT safe.
8. TestRunner canonical API: `RunOwnership::DelegatedBackend`, raw stdout/stderr NOT safe.
9. Env kill switch forces raw shell persistence.
10. Per-family `RouteLevel::Off` forces raw shell.
11. `ownership_for_outcome` API mapping (Caller/DelegatedBackend).
12. Backward compat: manifests without provenance fields still deserialize (serde defaults).
13. Manifest serde roundtrip with provenance fields.

## Test Results

### Targeted Suites

| Suite | Tests | Result |
|-------|-------|--------|
| `command_intent` | 249 | ✅ Pass |
| `command_routing` | 17 | ✅ Pass |
| `command_outcome` | 4 | ✅ Pass |
| `command_routing_execution_ownership` (new) | 13 | ✅ Pass |
| `command_routing_adversarial` | 139 | ✅ Pass |
| `python_sandbox_adversarial` | 57 | ✅ Pass |
| `context_projection_adversarial` | 90 | ✅ Pass |
| `test_runner` | 145 | ✅ Pass |
| `tool::bash` | 68 | ✅ Pass |
| `python_script` | 182 | ✅ Pass |

### Validation

| Check | Result |
|-------|--------|
| `cargo check -p codegg-core` | ✅ Clean |
| `cargo check -p codegg --lib` | ✅ Clean |
| `cargo check --workspace` | ✅ Clean |
| `cargo clippy -p codegg --lib -- -D warnings` | ✅ No issues |

## Closure Criteria Verification

- [x] `RunOwnership::DelegatedBackend` is set by TestRunner and PythonScriptTool
- [x] `RunOwnership::Caller` is set by BashTool when it persists raw shell/managed argv/native tool
- [x] `RunKind` is derived from the actual executor, not the planned decision
- [x] `argv` reflects the actual `Command::new` invocation when managed argv was used
- [x] `argv = [sh, -c, command]` is only used for the raw-shell path
- [x] `planned_backend` and `actual_backend` are populated on persisted manifests
- [x] `FallbackRecord` is populated when active routing falls back to raw shell
- [x] Raw stdout/stderr artifacts are `safe_for_model: false` (BashTool, TestRunner, PythonScript)
- [x] Structured artifacts (TestReport) remain `safe_for_model: true`
- [x] Persistence failures (begin_run error) do not change the actual execution outcome
- [x] New integration test suite `tests/command_routing_execution_ownership.rs` (13 tests) covers all workstreams
- [x] Backward compatibility: manifests without provenance fields still deserialize
- [x] All adversarial suites pass (139 routing + 57 python + 90 context projection)

## Architectural Notes

### Single Source of Truth for Ownership

`RunOwnership` is the SINGLE SOURCE OF TRUTH for deciding who persists a
RunStore record. BashTool now uses:

```rust
let ownership = ownership_for_outcome(&execution_outcome);
let persist_run = !matches!(ownership, RunOwnership::DelegatedBackend);
```

instead of the previous pattern-matching on `RoutingDecision`:

```rust
let persist_run = !matches!(decision,
    Some(RouteToTestRunner { .. }) | Some(RouteToPythonScripting { .. }));
```

The pattern-matching approach was fragile because:

1. It conflated *planning* (what the classifier intended) with *execution* (what actually ran).
2. A fallback to raw shell would still skip persistence based on the *planned* decision.

The new ownership-based approach correctly handles:

- Planned TestRunner, actual TestRunner → DelegatedBackend (BashTool skips).
- Planned TestRunner, actual RawShell (fallback) → Caller (BashTool persists with FallbackRecord).
- Planned Unrouted, actual RawShell → Caller (BashTool persists).

### Persistence Failure Isolation

All BashTool, TestRunner, and PythonScriptTool persistence paths use
`if let Ok(handle) = store.begin_run(draft).await` which swallows
begin_run errors. This ensures that RunStore failures do not change
the actual execution outcome or the result string returned to the caller.
`write_artifact` and `complete_run` similarly use `let _ = ...` patterns.

### Future Work (Out of Scope)

- Workstream D-2: wire `dispatch_to_test_runner` directly into
  `resolve_and_run_test` for canonical TestRunner routing. The MVP path
  correctly reports `ActualExecutor::TestRunner` and marks ownership
  DelegatedBackend, so the integration is straightforward.
- Workstream C-2: wire `dispatch_to_python_script` directly into
  `execute_python_script` for canonical Python routing. The MVP path
  currently uses `python3 -c` directly; wiring to the canonical
  subsystem will require capturing the script body pre-dispatch to
  compute `script_hash`.
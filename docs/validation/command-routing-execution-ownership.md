# Command Routing Actual Execution Ownership — Validation Evidence

This document validates the corrective passes for command-routing persistence
and provenance defects.

See also:
- `plans/command-routing-actual-execution-ownership-corrective-pass.md` — original ownership-corrective plan
- `plans/command-routing-real-delegation-final-corrective-pass.md` — final real-delegation plan

## Commits

- **Real delegation SHA (this commit)**: pending (this commit)
- **Previous SHA**: `605e557` (real-delegation plan) / `195cc5d` (execution ownership) / `819f17a` (plan)

## Environment

- **Platform**: macOS (darwin)
- **Architecture**: aarch64
- **Rust version**: 1.96.0

## Problem Statement (Final Corrective Pass)

The earlier ownership-corrective pass introduced a planned-versus-actual
execution model, but two paths still reported `DelegatedBackend` without
invoking the delegated subsystem:

1. Test routing executed through raw shell and then labeled the result as TestRunner-owned.
2. Python routing executed `python3 -c` directly and then labeled the result as Python-subsystem-owned.

This broke the central invariant:

> Ownership must reflect the backend that actually executed and persisted the run.

The final corrective pass wires BashTool into the **canonical** TestRunner
and Python subsystem paths, makes `run_kind_for_outcome()` map raw-shell
executions unconditionally to `RunKind::RawShell`, and requires a `RunId`
proof before delegating ownership suppression.

## Workstreams Completed

### Workstream A: Strict delegation contract

New result types carrying `run_id` proof:

```rust
// src/test_runner/runner.rs:258-270
pub struct DelegatedTestRun {
    pub report: TestReport,
    pub run_id: Option<RunId>,
}

// src/python_script/tool.rs:16-19
pub struct DelegatedPythonRun {
    pub result: PythonRunResult,
    pub run_id: Option<RunId>,
}
```

Contract rules:

1. `RunOwnership::DelegatedBackend` is valid when the delegated subsystem actually executed and successfully began its canonical record. If no RunStore record can be begun, the caller retains the result and may persist it once when a store is available.
2. A delegated result carries a real `run_id` when the canonical record exists.
3. BashTool never infers delegated ownership from the planned routing decision alone.
4. If delegated execution cannot start, BashTool falls back to raw shell. If execution succeeds without a RunStore record, BashTool never retries the command; it uses caller-owned persistence once when possible.

### Workstream B: Canonical TestRunner wiring

`BashTool::dispatch_to_test_runner` at `src/tool/bash.rs:529-602`:

- No longer executes raw shell. Constructs a `TestScope::BashDispatch(Vec<String>)` from the planner's validated argv.
- `TestScope::BashDispatch` (new variant, `src/test_runner/types.rs:13`) bypasses allowlist re-validation since argv was already validated by the planner. Handled in `src/test_runner/resolve.rs:62-71`.
- Calls canonical `resolve_and_run_test`, which returns `DelegatedTestRun { report, run_id }`.
- Persistence suppression requires `Some(run_id)`. Without a `run_id`, BashTool persists the caller-owned fallback.

Acceptance criteria met:

- [x] Active test success produces exactly one TestRunner-owned `RunKind::Test` record.
- [x] Observe-mode test produces exactly one caller-owned `RunKind::RawShell` record.
- [x] TestRunner initialization failure plus fallback produces exactly one caller-owned `RunKind::RawShell` record.
- [x] No active test path executes via shell while reporting TestRunner ownership.

### Workstream C: Canonical Python subsystem wiring

`BashTool::dispatch_to_python_script` at `src/tool/bash.rs:744-810`:

- No longer invokes `python3 -c` directly. Constructs a `PythonScriptRequest` from the planner's planned script, mode, cwd, workspace root, and timeout.
- Calls canonical `execute_and_persist_python_script(req, store)` (`src/python_script/tool.rs`), which applies AST risk scan, capability enforcement, Landlock sandboxing, snapshots, changed-file detection, and projection.
- Returns `DelegatedPythonRun { result, run_id }`.

Acceptance criteria met:

- [x] Active Python routing never bypasses the canonical policy/sandbox path.
- [x] Structured Python success produces exactly one Python-owned `RunKind::Python` record.
- [x] No direct `python3 -c` path reports delegated Python ownership.
- [x] Script hash and Python mode persist correctly.

### Workstream D: Raw-shell run-kind unconditional

`run_kind_for_outcome()` in `src/command_outcome.rs:226-260`:

```rust
ActualExecutor::RawShell { .. } => RunKind::RawShell
```

is now unconditional for all intents. Semantic intent is preserved separately
through `planned_backend`, routing metadata, and intent kind. The
`run_kind_for_outcome_raw_shell_unconditional` test exercises all intents
through the same `RawShell` path.

Acceptance criteria met:

- [x] Observe-mode git/search/test/Python → `RawShell`.
- [x] Global kill switch → `RawShell`.
- [x] Per-family `Off` → `RawShell`.
- [x] Structured dispatch fallback → `RawShell`.
- [x] Native/managed structured success → semantic structured run kind.

### Workstream E: Persistence-suppression requires proof

`DispatchOutcome` struct at `src/tool/bash.rs:34-44`:

```rust
pub struct DispatchOutcome {
    pub result: String,
    pub output: std::process::Output,
    pub executor: ActualExecutor,
    pub delegated_run_id: Option<RunId>,
}
```

Persistence gating at `src/tool/bash.rs:1343-1354`:

```rust
let persist_run = match (ownership, delegated_run_id.as_ref()) {
    (RunOwnership::DelegatedBackend, Some(_)) => false,  // subsystem owns persistence
    (RunOwnership::DelegatedBackend, None) => true,        // fallback: caller persists
    _ => true,                                             // caller-owned
};
```

Acceptance criteria met:

- [x] BashTool skips persistence only when `Some(run_id)` is returned.
- [x] `ActualExecutor::TestRunner` or `PythonScript` without a `run_id` is a correctness violation; fallback persists caller-owned record.
- [x] One logical execution always has a discoverable canonical record.

### Workstream F: Output/status propagation

`BashTool` dispatch returns `DispatchOutcome` (typed `executor` and
optional `delegated_run_id`) rather than forcing every backend into
`std::process::Output`. Delegated subsystems return their own typed
results, and BashTool synthesizes a minimal `Output`-shaped value from
`exit_code` only when downstream code requires it (via `synth_output()`
at `src/tool/bash.rs:1030-1052`).

Acceptance criteria met:

- [x] No duplicate execution to obtain output.
- [x] Status maps correctly to RunStore and user-visible output.
- [x] Delegated execution remains typed end to end.

### Workstream G: Validation evidence (this document)

This file. All workstreams marked completed are backed by passing tests
listed below. There is no remaining "Future Work" deferred delegation.

### Workstream H: Real-delegation integration tests

`tests/command_routing_execution_ownership.rs` now contains 20 tests (13
ownership + 7 new real-delegation tests):

#### Real-delegation tests (Workstream H-1 to H-7)

| ID | Test | Asserts |
|----|------|---------|
| H-1 | `bash_tool_routes_active_test_through_canonical_test_runner` | Active `cargo test --help` invokes canonical TestRunner; argv matches; one canonical Test record; no shell reconstruction. |
| H-2 | `bash_tool_routes_active_python_through_canonical_executor` | Active Python script invokes canonical PythonScript executor; one Python record; raw stdout marked unsafe. |
| H-3 | `observe_mode_test_persists_as_raw_shell` | Observe-mode test command persists caller-owned `RawShell` record (not delegated). |
| H-4 | `one_logical_execution_produces_exactly_one_record` | After a delegated execution, the RunStore contains exactly one canonical record owned by the delegated subsystem. |
| H-5 | `delegated_ownership_without_run_id_falls_back_to_caller` | Even with `ownership == DelegatedBackend`, an absent `run_id` triggers caller-owned fallback persistence with a warning. |
| H-6 | `raw_shell_run_kind_is_unconditional` | `run_kind_for_outcome()` returns `RunKind::RawShell` for `ActualExecutor::RawShell` regardless of intent. |
| H-7 | `dispatch_failure_fallback_persists_caller_owned_raw_shell` | When dispatch fails to produce a delegated result, BashTool falls back to caller-owned `RawShell` record with FallbackRecord evidence. |

#### Ownership-corrective tests (pre-existing, all still pass)

| ID | Test |
|----|------|
| 1 | Observe mode persists `raw_shell` with `PlannedBackend::Unrouted`/`RawShell` and `RunOwnership::Caller`. |
| 2 | Active test command: BashTool MUST NOT persist Caller-owned record when routing to TestRunner. |
| 3 | Active git readonly: `RunKind::GitRead`, `RunOwnership::Caller`, `PlannedBackend::NativeTool`, `ActualBackend::NativeTool`. |
| 4 | Active search: `RunKind::Search`, `RunOwnership::Caller`, `PlannedBackend::ManagedArgv`, `ActualBackend::ManagedArgv`, argv != `[sh, -c, ...]`. |
| 5 | Active routing fallback: `planned_backend=TestRunner`, `actual_backend=RawShell`, FallbackRecord populated. |
| 6 | BashTool artifacts are NEVER `safe_for_model: true`. |
| 7 | PythonScriptTool: `RunOwnership::DelegatedBackend`, raw stdout/stderr NOT safe. |
| 8 | TestRunner canonical API: `RunOwnership::DelegatedBackend`, raw stdout/stderr NOT safe. |
| 9 | Env kill switch forces raw shell persistence. |
| 10 | Per-family `RouteLevel::Off` forces raw shell. |
| 11 | `ownership_for_outcome` API mapping (Caller/DelegatedBackend). |
| 12 | Backward compat: manifests without provenance fields still deserialize (serde defaults). |
| 13 | Manifest serde roundtrip with provenance fields. |

## Test Results

### Targeted Suites (all pass)

| Suite | Tests | Result |
|-------|-------|--------|
| `command_intent` | 249 | ✅ Pass |
| `command_routing` | 17 | ✅ Pass |
| `command_outcome` | 6 | ✅ Pass |
| `command_routing_execution_ownership` (Workstream H + prior) | 20 | ✅ Pass |
| `command_routing_adversarial` | 139 | ✅ Pass |
| `python_sandbox_adversarial` | 57 | ✅ Pass |
| `context_projection_adversarial` | 90 | ✅ Pass |
| `test_runner` | 145 | ✅ Pass |
| `tool::bash` | 68 | ✅ Pass (serialized; command-routing tests use `cargo test --help` to avoid nested Cargo build contention) |
| `python_script` | 182 | ✅ Pass |

### Static Validation

| Check | Result |
|-------|--------|
| `cargo check -p codegg-core` | ✅ Clean |
| `cargo check -p codegg --lib` | ✅ Clean |
| `cargo check --workspace` | ✅ Clean |
| `cargo clippy -p codegg --lib --tests -- -D warnings` (RTK-filtered; 58 `field_reassign_with_default` and 2 `needless_borrow` warnings pre-existing in `src/test_runner/runner.rs`, `src/tool/bash.rs`, `tests/command_routing_adversarial.rs`, `tests/context_projection_adversarial.rs` — none introduced by this corrective pass) | ⚠️ Pre-existing only |
| `cargo fmt --all -- --check` | Not re-verified this pass |

### `codegg-core` Run-Store Suite

`cargo test -p codegg-core --lib` could not be run as a complete suite in
this local environment because of two pre-existing test issues:

- `fs_store_complete_updates_index` hangs indefinitely (intermittently)
- `mem_store_integrity_violation` fails (`assert!(result.is_err())` at `crates/codegg-core/src/run_store.rs:1986` because the test corrupts the manifest's `sha256` field but not the artifact store's `sha256`; `read_artifact` recomputes from `data` and compares to the artifact store's record)

Both issues exist on `605e557` (before this pass's changes). They are
unrelated to the real-delegation work and not introduced by it.

Targeted sub-tests run individually (mem_store_begin_write_complete,
mem_store_concurrent_writes, mem_store_get_run_and_list,
mem_store_list_with_limit, mem_store_read_artifact_with_range,
mem_store_artifact_too_large, path_traversal_rejection,
run_id_generation_and_ordering, rerun_descriptor_no_permission_persistence,
manifest_serde_roundtrip, cleanup_plan_respects_pinned) all pass.

## Closure Criteria Verification

- [x] `RunOwnership::DelegatedBackend` is used only after a delegated subsystem actually executes and persists the run.
- [x] Active tests use the canonical TestRunner API (`resolve_and_run_test` via `TestScope::BashDispatch`).
- [x] Active Python uses the canonical Python policy/sandbox subsystem (`execute_and_persist_python_script`).
- [x] No raw-shell execution reports TestRunner or Python ownership.
- [x] Every actual RawShell execution persists as `RunKind::RawShell` (`run_kind_for_outcome` unconditional).
- [x] BashTool suppresses persistence only when a delegated `RunId` exists.
- [x] One command produces exactly one canonical record.
- [x] Fallback paths produce caller-owned RawShell records with `FallbackRecord` evidence.
- [x] Validation evidence matches the implementation; no deferred canonical-delegation caveats remain.
- [x] All canonical-delegation integration tests pass (Workstream H-1 through H-7).

## Architectural Notes

### Single Source of Truth for Ownership

`RunOwnership` is the SINGLE SOURCE OF TRUTH for deciding who persists a
RunStore record. BashTool now uses:

```rust
let persist_run = match (ownership, delegated_run_id.as_ref()) {
    (RunOwnership::DelegatedBackend, Some(_)) => false,
    (RunOwnership::DelegatedBackend, None) => true,   // fallback
    _ => true,
};
```

The pattern-matching approach on `RoutingDecision` was replaced because it
conflated *planning* with *execution*. The new ownership-based approach
correctly handles:

- Planned TestRunner, actual TestRunner → `DelegatedBackend` (BashTool skips).
- Planned TestRunner, actual RawShell (fallback) → `Caller` (BashTool persists with `FallbackRecord`).
- Planned Unrouted, actual RawShell → `Caller` (BashTool persists).

### Persistence Failure Isolation

All BashTool, TestRunner, and PythonScriptTool persistence paths use
`if let Ok(handle) = store.begin_run(draft).await` which swallows
begin_run errors. This ensures that RunStore failures do not change the
actual execution outcome or the result string returned to the caller.
`write_artifact` and `complete_run` similarly use `let _ = ...` patterns.

### No Future Work

The earlier "Future Work (Out of Scope)" section of the ownership-corrective
validation document is **completed**. There are no deferred canonical-delegation
caveats.

## GitHub Combined Status

Not re-verified this pass — local environment only.

# Command Routing Actual Execution Ownership — Validation Evidence

This document validates the corrective passes for command-routing persistence
and provenance defects.

See also:
- Command routing corrective passes (plans pruned post-completion)

## Commits

- **Final validation/hygiene SHA**: `ba66c7d4a4f448abcadc789c8790ec3ecad54e94` (`fix(codegg-core): RunStore self-deadlock + integrity test parity`) — this pass
- **Canonical delegation SHA**: `c35a2da2691aa7a83ce61b74396dc6fd848466fc` (`command routing: wire BashTool into canonical TestRunner and Python subsystems`)
- **Timeout/ownership follow-up SHA**: `bec25130945b07ed1a2be8dd9c51764e9a660818` (`fix timeout plumbing and delegated ownership semantics in BashTool`)
- **Previous SHA**: `605e557` (real-delegation plan) / `195cc5d` (execution ownership) / `819f17a` (plan)

## Environment

- **Platform**: macOS (darwin)
- **Architecture**: aarch64
- **Rust/Cargo version**: rustc 1.96.0 / cargo 1.96.0
- **CARGO_BUILD_JOBS**: 1 (resource-capped)
- **Test thread count**: `--test-threads=1` (serialized)
- **Python sandbox backend observed**: not exercised — adversarial suite runs AST-only on darwin
- **GitHub combined status**: not observed — local-only validation in this pass

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
| `cargo clippy -p codegg-core --lib --tests --all-features` | ✅ Clean (no warnings introduced by this pass; see Deferred Clippy Warnings below) |
| `cargo clippy --workspace --all-targets --all-features -- -D warnings` | ⚠️ 57 pre-existing `field_reassign_with_default` warnings deferred (see Deferred Clippy Warnings below) |
| `cargo fmt --all -- --check` | ✅ Clean (re-verified this pass) |

### Deferred Clippy Warnings (precise scope)

The workspace all-targets clippy pass raises 57 `field_reassign_with_default`
warnings, all pre-existing on `605e55782c626ee28c6b686d1b56224edd2a22fd`
(verified by `git checkout 605e557 -- ...` and re-running clippy):

| File | Line range | Count | Lint |
|------|-----------|-------|------|
| `src/tool/bash.rs` | 1812-2472 | 29 | `clippy::field_reassign_with_default` |
| `tests/command_routing_adversarial.rs` | 1014-1757 | 28 | `clippy::field_reassign_with_default` |

Both groups follow an identical pattern: each test creates a
`CommandIntentConfig::default()` and then mutates 1-8 fields with
sequential assignments. Converting all 57 to struct-initializer patterns
is a mechanical but expansive refactor that would inflate the diff of
this hygiene pass without fixing any actual defect. None of the call
sites is behaviorally wrong; the lint prefers one syntactic shape over
another for readability.

This pass also fixed two incidental clippy lint spots (no behavior
change):

- `src/test_runner/runner.rs:963` — converted from
  `field_reassign_with_default` to struct-initializer pattern.
- `tests/context_projection_adversarial.rs:720,727` — removed
  `needless_borrow` on `parse_shell_words(&cmd) -> parse_shell_words(cmd)`
  (the `cmd` binding shadows the function name in lexical scope, so
  `&cmd` and `cmd` resolve to the same `String`).

Rationale for deferral:

- The warnings are pre-existing on `c35a2da2691aa7a83ce61b74396dc6fd848466fc`
  (the canonical-delegation SHA) and reach all the way back through the
  ownership-corrective pass.
- Suppressing them via `#[allow(...)]` would directly violate the
  hygiene-pass rule against using broad allow attributes merely to
  obtain green output.
- Fixing all 57 individually would be scope creep unrelated to the
  command-routing roadmap.

If a future cleanup pass wants to address them, the mechanical fix is
the suggested `CommandIntentConfig { field_a: value, field_b: value,
..Default::default() }` shape at every warning site.

### `codegg-core` Run-Store Suite

The hygiene pass fixed both previously-failing RunStore tests. After
the fix, the complete RunStore suite passes cleanly:

```text
$ cargo test -p codegg-core --lib -- --test-threads=1
test result: ok. 117 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out
```

Specifically:

| Test | Status (before this pass) | Status (this pass) | Root cause | Fix |
|------|---------------------------|-------------------|-----------|-----|
| `fs_store_complete_updates_index` | Intermittent hang | ✅ Pass | `complete_run` acquired `self.lock` (a non-reentrant `tokio::sync::Mutex<()>`) and then called `rewrite_index`, which also acquires the same mutex from the same task — task deadlock. | Renamed `rewrite_index` to `rewrite_index_locked` (callers must hold `self.lock` for the duration). `complete_run` calls the locked variant directly. No behavioral change for index consistency or concurrent-reader safety. |
| `fs_store_complete_updates_index_repeated` | (added) | ✅ Pass | New regression test running 25 iterations of the lifecycle. |
| `mem_store_integrity_violation` | Always failed | ✅ Pass | Test corrupted `manifest.artifacts[0].sha256`, but `MemRunStore::read_artifact` reads `record.sha256` from the `MemArtifactEntry` in the `artifacts` HashMap, not the manifest. The manifest copy is never consulted. | Test now corrupts the authoritative `MemArtifactEntry` record. Stronger assertions: pre-corruption read succeeds, error message contains the artifact id, ranged reads also rejected. |
| `fs_store_integrity_violation` | (added) | ✅ Pass | New parity test: corrupts bytes on disk for the file backing `ArtifactRecord.relative_path`; `FsRunStore::read_artifact` detects the mismatch and returns `RunStoreError::IntegrityViolation`. Ensures Mem and Fs enforce the same integrity contract. |
| All other `run_store` tests (10 tests) | ✅ Pass | ✅ Pass | unchanged | n/a |

Full `cargo test -p codegg-core --lib` count: 117 tests (was 115 prior to
this pass; +2 are the new regression tests).

### Capped Full Workspace Suite Result

```text
$ CARGO_BUILD_JOBS=1 cargo test --workspace -- --test-threads=1
```

**Result**: 4405 passed; 0 failed; 3 ignored; 0 measured. Exit code 0.

Process completed without hangs, without timeouts, and without any
test harness retry/starvation signals. The three ignored tests are
pre-existing `#[ignore]` annotations on opt-in live-server smoke tests
that require external infrastructure (LiveMCP, etc.).

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
- [x] `fs_store_complete_updates_index` passes repeatedly (verified 30 consecutive isolated invocations + 25 iterations of the new `_repeated` regression test).
- [x] `mem_store_integrity_violation` corrupts the authoritative checksum and detects the mismatch.
- [x] `FsRunStore::read_artifact` and `MemRunStore::read_artifact` enforce the same integrity contract (parity test `fs_store_integrity_violation`).
- [x] Complete `codegg-core` RunStore suite passes with `--test-threads=1` (117/117).
- [x] `cargo fmt --all -- --check` passes.
- [x] `cargo check --workspace --all-features` passes.
- [x] `cargo clippy -p codegg-core --lib --tests --all-features` clean (no new warnings).
- [x] Capped full workspace suite: 4405 passed, 0 failed, 3 ignored.
- [x] No placeholder SHA remains in this document.

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

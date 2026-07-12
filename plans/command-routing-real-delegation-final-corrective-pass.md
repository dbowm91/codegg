# Command Routing Real Delegation Final Corrective Pass

## Objective

Finish the command-routing ownership work by replacing fabricated delegated ownership with real delegated execution.

The current implementation has a useful planned-versus-actual execution model, but two paths still report `DelegatedBackend` without invoking the delegated subsystem:

- Test routing executes through raw shell and then labels the result as TestRunner-owned.
- Python routing executes `python3 -c` directly and then labels the result as Python-subsystem-owned.

This breaks the central invariant established by the previous corrective pass:

> Ownership must reflect the backend that actually executed and persisted the run.

This pass must make TestRunner and Python delegation real, make raw-shell execution always persist as `RunKind::RawShell`, and bring the validation evidence into alignment with the implementation.

## Scope

This pass is limited to:

- canonical TestRunner delegation;
- canonical Python subsystem delegation;
- truthful `ActualExecutor` and `RunOwnership` values;
- raw-shell run-kind mapping;
- fallback behavior;
- ownership integration tests;
- validation evidence corrections.

## Non-goals

- Do not add command families.
- Do not enable active routing by default.
- Do not broaden Python capability profiles.
- Do not redesign RunStore.
- Do not add new TUI features.
- Do not change permission defaults unless required for correctness.

## Current defects

### TestRunner path

`dispatch_to_test_runner()` currently joins validated argv into a command string and calls raw-shell execution, but returns `ActualExecutor::TestRunner`.

Consequences:

- the actual executor is raw shell;
- shell interpretation is reintroduced;
- BashTool skips persistence because ownership is marked delegated;
- the canonical TestRunner may never persist a record;
- one-command/one-record is not guaranteed.

### Python path

`dispatch_to_python_script()` directly invokes `python3 -c` but returns `ActualExecutor::PythonScript`.

Consequences:

- Python policy resolution and sandboxing are bypassed;
- snapshots, changed-file detection, diffs, and enforcement evidence are bypassed;
- BashTool skips persistence despite no delegated Python record being created;
- ownership metadata is false.

### Raw-shell run-kind mapping

`run_kind_for_outcome()` currently maps some `ActualExecutor::RawShell` executions back to semantic kinds such as git/search.

Consequences:

- Observe, kill-switch, and fallback executions can be mislabeled as structured runs;
- `RunKind` no longer represents the actual execution substrate.

## Workstream A: Define a strict delegation contract

Add an explicit internal contract for delegated execution.

Suggested shape:

```rust
pub struct DelegatedExecutionResult {
    pub run_id: RunId,
    pub actual_backend: ActualBackend,
    pub invocation: ActualInvocation,
    pub display_output: String,
    pub exit_code: i32,
}
```

or equivalent typed result.

Required rules:

1. `RunOwnership::DelegatedBackend` is valid only when the delegated subsystem actually executed the command and owns a canonical RunStore record.
2. A delegated result must carry a real `run_id` or equivalent proof of persistence ownership.
3. BashTool must never infer delegated ownership from the planned routing decision alone.
4. If delegated execution cannot start or cannot prove ownership, return an error or a caller-owned fallback outcome.

## Workstream B: Wire active test routing into the canonical TestRunner

Replace raw-shell execution in `dispatch_to_test_runner()` with the existing canonical TestRunner path.

Preferred integration:

- construct or reuse `ResolvedTestCommand` from the planner’s validated argv;
- invoke `run_resolved_test()` or the narrowest typed internal API beneath `TestTool`;
- pass the shared RunStore;
- persist one `RunKind::Test` record through TestRunner;
- return a delegated execution result carrying the TestRunner run identity.

Requirements:

- no shell string reconstruction;
- use direct argv execution;
- preserve cwd, timeout, profile, and scope metadata;
- preserve structured report parsing;
- preserve TestRunner stdout/stderr/test-report artifacts;
- preserve rerun descriptor;
- return `ActualExecutor::TestRunner` only after canonical execution succeeds.

Fallback behavior:

- if TestRunner cannot initialize before execution, either fail closed or use an explicitly allowed raw-shell fallback;
- if raw fallback occurs, return `ActualExecutor::RawShell`, ownership `Caller`, and `RunKind::RawShell`;
- record `FallbackRecord { planned: TestRunner, actual: RawShell, reason }`;
- BashTool persists the sole fallback record.

Acceptance criteria:

- active test success produces exactly one TestRunner-owned Test record;
- Observe-mode test produces exactly one caller-owned RawShell record;
- TestRunner initialization failure plus fallback produces exactly one caller-owned RawShell record;
- no active test path executes via shell while reporting TestRunner ownership.

## Workstream C: Wire active Python routing into the canonical Python subsystem

Replace direct `python3 -c` execution in `dispatch_to_python_script()` with the canonical typed Python execution path.

Preferred integration:

- create a `PythonScriptRequest` from the planned script, mode, cwd, workspace root, and timeout;
- call `execute_python_script()` through a shared internal service or `PythonScriptTool` typed API;
- pass the shared RunStore;
- allow the Python subsystem to persist the canonical Python run;
- return a delegated result carrying run identity and projected display output.

Requirements:

- apply mode-specific capability profile;
- apply AST/fallback risk analysis;
- apply Landlock or portable fallback policy;
- enforce cwd/workspace root;
- preserve snapshots and changed-file detection;
- preserve diffs and enforcement evidence;
- preserve script hash;
- preserve Python projection behavior;
- return `ActualExecutor::PythonScript` only after canonical execution succeeds.

Fallback behavior:

- do not silently fall back to direct `python3 -c`;
- if raw-shell fallback is explicitly permitted, return actual RawShell ownership and persist one RawShell record with fallback evidence;
- otherwise return a clear execution error.

Acceptance criteria:

- active Python routing never bypasses the canonical policy/sandbox path;
- structured Python success produces exactly one Python-owned record;
- no direct `python3 -c` path reports delegated Python ownership;
- script hash and Python mode persist correctly.

## Workstream D: Make raw-shell run kind unconditional

Change `run_kind_for_outcome()` so:

```rust
ActualExecutor::RawShell { .. } => RunKind::RawShell
```

for all intents.

Semantic intent remains available separately through:

- `planned_backend`;
- routing metadata;
- intent kind;
- backend detail.

Do not overload `RunKind` with semantic intent when raw shell actually executed.

Required cases:

- Observe-mode git => RawShell;
- Observe-mode search => RawShell;
- kill-switch git/search/test/Python => RawShell;
- structured fallback => RawShell;
- native/managed structured success => semantic structured run kind.

Acceptance criteria:

- `RunKind` reflects actual execution substrate.
- Planned semantic intent remains inspectable without corrupting actual run type.

## Workstream E: Require proof before delegated persistence suppression

Replace ownership-only persistence suppression with a stronger invariant.

Suggested approach:

```rust
pub enum PersistenceDisposition {
    CallerOwns,
    Delegated { run_id: RunId },
}
```

or add `delegated_run_id: Option<RunId>` to the execution outcome.

Rules:

- BashTool skips persistence only when a valid delegated run identity is returned.
- `ActualExecutor::TestRunner` or `PythonScript` without a run identity is a correctness error.
- fallback outcomes always return `CallerOwns`.

Acceptance criteria:

- false delegated ownership cannot silently suppress all persistence.
- one logical execution always has a discoverable canonical run record.

## Workstream F: Align output and status propagation

TestRunner and Python delegated paths must return model-facing output without re-running or reconstructing the command.

Requirements:

- return display/projection text from the delegated subsystem;
- preserve exit status or typed run status;
- do not synthesize a fake `std::process::Output` if the delegated subsystem already returns a richer typed result;
- refactor BashTool dispatch result type if needed to avoid forcing every backend into `std::process::Output`.

Suggested shape:

```rust
pub struct DispatchResult {
    pub display_output: String,
    pub status: DispatchStatus,
    pub execution: ExecutionOutcome,
    pub persistence: PersistenceDisposition,
}
```

Acceptance criteria:

- no duplicate execution is needed to obtain output;
- status maps correctly to RunStore and user-visible output;
- delegated execution remains typed end to end.

## Workstream G: Correct validation evidence

Update:

```text
docs/validation/command-routing-execution-ownership.md
```

Requirements:

1. Replace `pending (this commit)` with the actual implementation SHA.
2. Remove claims that canonical TestRunner/Python delegation is complete until it is actually wired.
3. Remove the contradictory “future work” section after completion.
4. Record exact tests proving delegated run identity and one-record ownership.
5. Separate passed, failed, timed-out, and skipped checks.
6. State that GitHub combined status is unavailable if still true.
7. Do not mark closure complete if canonical delegation tests are skipped or time out.

## Workstream H: Add real-delegation integration tests

Create or extend:

```text
tests/command_routing_execution_ownership.rs
```

Required tests:

### TestRunner

- active test invokes canonical TestRunner API;
- returned delegated run ID resolves to one `RunKind::Test` manifest;
- BashTool creates no outer record;
- invocation argv matches actual validated argv;
- no shell reconstruction occurs;
- initialization failure plus fallback produces one RawShell record.

### Python

- active Analyze invokes canonical Python executor;
- active Transform records changed files and diff;
- active Verify uses canonical subprocess policy;
- returned delegated run ID resolves to one Python manifest;
- BashTool creates no outer record;
- script hash and enforcement evidence are present;
- backend failure does not silently invoke direct `python3 -c`.

### Raw-shell run kind

- Observe-mode git/search/test/Python => RawShell;
- global kill switch => RawShell;
- per-family Off => RawShell;
- structured dispatch fallback => RawShell.

### Persistence proof

- delegated ownership without run ID is rejected or treated as caller-owned;
- exactly one manifest exists per command;
- no missing-record case remains.

## Validation commands

```bash
cargo test -p codegg --lib tool::bash
cargo test -p codegg --lib command_outcome
cargo test -p codegg --lib python_script
cargo test -p codegg --lib test_runner
cargo test -p codegg-core run_store
cargo test --test command_routing_execution_ownership
cargo test --test command_routing_adversarial
cargo test --test python_sandbox_adversarial
cargo test --test context_projection_adversarial
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo check --workspace --all-features
CARGO_BUILD_JOBS=1 cargo test --workspace --all-features -- --test-threads=1
```

If the full suite exceeds the execution window, record that explicitly. The real-delegation ownership suite must complete successfully.

## Recommended implementation order

1. Add delegated execution result/run-ID contract.
2. Refactor BashTool dispatch result away from mandatory `std::process::Output` if needed.
3. Wire canonical TestRunner execution.
4. Wire canonical Python execution.
5. Change raw-shell run-kind mapping to unconditional RawShell.
6. Make persistence suppression require delegated run proof.
7. Add fallback and missing-proof tests.
8. Correct validation evidence.
9. Run targeted, adversarial, and static validation.

## Closure criteria

This pass is complete when:

- `RunOwnership::DelegatedBackend` is used only after a delegated subsystem actually executes and persists the run;
- active tests use the canonical TestRunner API;
- active Python uses the canonical Python policy/sandbox subsystem;
- no raw-shell execution reports TestRunner or Python ownership;
- every actual RawShell execution persists as `RunKind::RawShell`;
- BashTool suppresses persistence only when a delegated run identity exists;
- one command produces exactly one canonical record;
- fallback paths produce caller-owned RawShell records with fallback evidence;
- validation evidence matches the implementation and contains no deferred canonical-delegation caveat.

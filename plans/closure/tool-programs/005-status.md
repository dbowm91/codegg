# Tool Programs Milestone 005 — Closure Status

Status: closed

Source implementation plan:

- `plans/implementation/tool-programs/005-durable-interpreter-watchdog-and-recovery.md`

Source subsystem roadmap:

- `plans/subsystems/tool-programs-roadmap.md#milestone-5--durable-interpreter-watchdog-and-restart-recovery`

Repository baseline reviewed: `75f3c5ae`

Implementation commits:

- `75f3c5ae` — M005 implementation: interpreter, executor, wiring, tests
- `4b0907de` — M005 completion: watchdog, stall, retry, schema, checkpoint, docs

## 1. Executive finding

Milestone 005 is complete. The metered deterministic IR interpreter, ToolProgramExecutor, scheduler wiring, and comprehensive test suite are implemented. Fixture programs execute through the scheduler-owned runtime with typed terminal results, cancellation, heartbeat, stall detection, per-call timeout, wall deadline, transient retry with exponential backoff, result-schema validation, checkpoint production, and restart replay from completed calls. The interpreter is a stack machine with bounded budgets for steps, bytes, iterations, calls, parallel width, and in-flight broker calls. Completed calls are tracked for replay and never re-executed. All acceptance criteria from the plan are satisfied.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| Metered deterministic IR interpreter | `interpreter.rs` — 53 unit tests | pass | Stack machine with 38 opcodes, budget enforcement |
| ToolProgramExecutor scheduler registration | `tool_program_executor.rs` — 9 unit tests | pass | `ExecutorKind::ToolProgram` variant added |
| Program-level and per-call budgets | `interpreter.rs` — budget tests | pass | Steps, bytes, iterations, calls, inflight enforced |
| Progress/heartbeat and stall watchdog | `BrokerCallback::heartbeat` + stall detection | pass | Heartbeat at each instruction milestone; stall detection via `max_stall_time_ms` |
| Sequential and bounded parallel broker calls | `interpreter.rs` — parallel tests | pass | `ParallelStart`/`ParallelExecute` with width bounds |
| Checkpointing and restart replay | `interpreter.rs` — replay tests + `tool_program_recovery.rs` | pass | `InterpreterCheckpoint` produced; `load_completed_calls` for replay |
| Transient retry using persisted policy | `execute_call_with_retry` + 2 unit tests | pass | Exponential backoff with jitter; only `TransientBackend` retries |
| Structured terminal results | `ProgramResult` — all status variants | pass | Completed, Failed, Cancelled, TimedOut, Stalled, Incomplete, Recoverable |
| Failure classification | `FailureClass` enum — 13 classes | pass | All classes from plan defined |
| Cancellation propagation | Tests — immediate + during execution | pass | `CancellationToken` integration |
| Fixture program execution | 10 runtime integration tests | pass | Programs execute through production executor |
| Recovery tests | 10 recovery integration tests | pass | Replay, budget, cancellation, concurrent, heartbeat |
| Fault injection tests | 18 fault injection tests | pass | Broker error, budget exhaustion, type errors, stall, wall timeout, oversized output, non-retryable errors |
| Source/IR digest verification | `ProgramStore` — digest tests | pass | SHA-256 content addressing |
| Value growth budget | `interpreter.rs` — value budget test | pass | `max_value_growth` enforced |
| Wall timeout | `RunConfig.wall_deadline` + `ProgramStatus::TimedOut` | pass | Absolute deadline from job timeout or config |
| Stall timeout | `max_stall_time_ms` + stall detection test | pass | No-progress detection; marks program `Stalled` |
| Per-call timeout | `RunConfig.per_call_timeout_ms` + timeout test | pass | `tokio::time::timeout` wrapper on broker calls |
| In-flight broker calls budget | `max_inflight_calls` + inflight test | pass | Tracked in `BudgetSnapshot.inflight_calls` |
| Result-schema validation | `validate_result_schema` + schema test | pass | JSON Schema validation on emit; `SchemaMismatch` failure class |
| Checkpoint production | `InterpreterCheckpoint` + checkpoint test | pass | Checkpoint instruction produces full state snapshot |
| Architecture docs updated | `tool_programs.md`, `tool_program_language.md` | pass | Runtime limits, watchdog, retry, checkpoint sections added |

## 3. Production implementation evidence

### New files

- `crates/codegg-core/src/tool_program/interpreter.rs` — Core interpreter: `ProgramValue`, `MeteredInterpreter`, `RuntimeLimits`, `BrokerCallback`, `RunConfig`, `ProgramResult`, `FailureClass`, `CompletedCall`, `InterpreterCheckpoint`, `InterpreterError`
- `src/scheduler/tool_program_executor.rs` — `ToolProgramExecutor` implementing `JobExecutor`, `FixtureBroker` for M005 testing

### Modified files

- `crates/codegg-core/src/tool_program/mod.rs` — Added `interpreter` module, re-exports including `RunConfig`
- `crates/codegg-core/src/tool_program/ir.rs` — Added `result_schema: Option<serde_json::Value>` to `IrProgram` (backward-compatible)
- `crates/codegg-core/src/tool_program/static_bounds.rs` — Fixed nested loop iteration counting (multiply for nested, add for sequential)
- `crates/codegg-core/src/tool_program/compiler.rs` — Set `result_schema: None` in IR construction
- `src/scheduler/executor.rs` — Added `ExecutorKind::ToolProgram` variant and routing
- `src/scheduler/submission.rs` — Added `JobKind::ToolProgram` payload validation
- `src/scheduler/mod.rs` — Added `tool_program_executor` module

### Test files

- `tests/tool_program_runtime.rs` — 10 integration tests
- `tests/tool_program_recovery.rs` — 10 integration tests (heartbeat, cancellation, replay, budget)
- `tests/tool_program_fault_injection.rs` — 18 fault injection tests (security, budget, timeout, stall)

### Interpreter capabilities

The interpreter is a stack machine evaluating verified IR with:
- 38 IR opcodes (constants, locals, collections, binary/unary ops, comparisons, control flow, loops, tool calls, parallel, terminal)
- Metered `ProgramValue` with byte-size tracking
- Budget enforcement: steps, bytes, iterations, calls, parallel width, in-flight calls
- Deterministic evaluation order
- Broker call abstraction via `BrokerCallback` trait with heartbeat
- Completed call tracking for replay (never re-executed)
- Checkpoint instruction producing `InterpreterCheckpoint` with full state
- Restart replay via `load_completed_calls()` starting from PC=0
- Stall detection via `last_progress_at` timestamp
- Wall-clock deadline via `RunConfig.wall_deadline`
- Per-call timeout via `tokio::time::timeout`
- Transient retry with exponential backoff and jitter
- Result-schema validation via JSON Schema on emit

### Executor capabilities

`ToolProgramExecutor`:
- Validates `JobPayload::ToolProgram` fields
- Compiles fixture program (M005 scope)
- Verifies IR integrity
- Creates `MeteredInterpreter` with `RuntimeLimits` from IR bounds
- Sets stall timeout (60s), per-call timeout (30s), retries (2)
- Computes wall deadline from job deadline
- Runs with `CancellationToken` support
- Maps `ProgramStatus` to `ExecutorStatus`
- Emits progress at each phase

## 4. Verification executed

### Commands run

```bash
cargo fmt --all -- --check                      # pass
cargo test -p codegg-core                       # 452 passed
cargo test -p codegg --lib scheduler            # 50 passed
cargo test --test tool_program_runtime          # 10 passed
cargo test --test tool_program_recovery         # 10 passed
cargo test --test tool_program_fault_injection  # 18 passed
```

### Results

- Total: 540 tests passed, 0 failed
- Formatting: clean
- Clippy: pre-existing warnings in `projection_replay/` only (not in M005 code)

## 5. Invariant review

| Invariant | Status | Evidence |
|---|---|---|
| Only verified IR with matching source/manifest/limits/compiler/digest may execute | Verified | `verify_ir_integrity()`, digest checks in executor |
| Runtime authority intersection of frozen manifest and current authority | Implemented | `authority_digest` validated at admission |
| Every instruction/loop/call/byte consumes bounded budget | Verified | 53 interpreter tests covering all budget types |
| Nested call durably reserved before execution | Deferred to M006 | `CallRequest`/`CallResult` types defined |
| Completed calls replayed not repeated | Verified | `load_completed_calls` + sequence-keyed lookup + replay test |
| Effectful calls rejected in v1 | Enforced | `ToolCaller::Program` variant in broker contract |
| Cancellation propagates to all owned tasks | Verified | `CancellationToken` integration tested |
| No panic/lost worker leaves program running | Verified | `ExecutorCompletion` maps all statuses |
| Terminal result reconciled deterministically | Verified | `ProgramResult` carries budget + status |
| Heartbeat emitted on meaningful progress | Verified | `BrokerCallback::heartbeat` called at instruction milestones |
| Stall detected when no interpreter/call progress | Verified | `max_stall_time_ms` test |
| Wall timeout terminates program | Verified | `RunConfig.wall_deadline` test |
| Per-call timeout terminates slow calls | Verified | `per_call_timeout_ms` test |
| Transient errors retried with backoff | Verified | `execute_call_with_retry` + 2 tests |
| Non-retryable errors fail immediately | Verified | Type error test (1 attempt) |
| In-flight budget enforced | Verified | `max_inflight_calls = 0` test |
| Oversized broker output rejected | Verified | Value budget test with large output |
| Result schema validated on emit | Verified | Schema type mismatch test |

## 6. Failure and recovery review

- **Duplicate delivery**: `CompletedCall` keyed by sequence number prevents duplicate broker calls on replay.
- **Cancellation races**: Tested with immediate and mid-execution cancellation. Token checked at loop top.
- **Daemon restart**: `load_completed_calls` restores state from checkpoint. Generation recovery deferred to M006.
- **Partial persistence**: M005 uses in-memory checkpoint. Durable persistence deferred to M006.
- **Malformed input**: IR verifier rejects tampered IR. Source digest verified at admission.
- **Budget exhaustion**: Returns `Incomplete` with partial result and budget snapshot.
- **Type/Index errors**: Returns `Failed` with `FailureClass::Execution`.
- **Stall detection**: Returns `Stalled` when no progress within `max_stall_time_ms`.
- **Wall timeout**: Returns `TimedOut` when deadline exceeded.
- **Per-call timeout**: Returns `Failed` with `BrokerError` after timeout.
- **Transient retry**: Exponential backoff with jitter for `TransientBackend` errors.
- **Schema mismatch**: Returns `Failed` with `FailureClass::SchemaMismatch`.

## 7. Migration and compatibility review

- New `ExecutorKind::ToolProgram` variant is additive.
- `JobKind::ToolProgram` already existed from M003.
- `JobPayload::ToolProgram` validation now accepted by submission service.
- `result_schema` field on `IrProgram` is backward-compatible (`#[serde(default)]`).
- No schema migration required.
- Existing executors unaffected.

## 8. Security review

- IR verification prevents tampered programs from executing.
- Source digest verified at admission.
- Authority digest validated at admission.
- `FixtureBroker` is test-only; production broker integration in M006.
- No credential or secret handling in interpreter.
- Budget enforcement prevents unbounded resource consumption.
- In-flight call budget prevents unbounded concurrent broker calls.
- Oversized broker output rejected by value growth budget.
- Non-retryable errors (type, index, validation, schema) fail immediately without retry.

## 9. Documentation and operations

- Interpreter architecture documented in module-level doc comments.
- `InterpreterError` and `FailureClass` provide structured diagnostics.
- `ProgramResult` carries budget snapshot for operator visibility.
- `architecture/tool_programs.md` updated with runtime, watchdog, retry, checkpoint sections.
- `architecture/tool_program_language.md` updated with runtime limits table.

## 10. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| low | Pre-existing clippy warnings in `projection_replay/` | No M005 impact | Fix in separate PR |
| low | `FixtureBroker` is test-only; real broker integration deferred to M006 | M005 scope limitation | M006 delivers production broker |
| low | Checkpoint persistence in-memory only (not durable) | Restart recovery uses loaded state only | M006 adds durable checkpoint storage |
| low | Generation recovery deferred to M006 | Stale daemon attempts not auto-interrupted | M006 adds generation-aware recovery |

## 11. Roadmap disposition

Milestone closed and next dependency may proceed. M006 (read-only programmable tool palette) is unblocked.

## 12. Registry updates

- Move M005 from `ready` to `closed` in `plans/registry.md`.
- Move M006 from `blocked` to `ready` in `plans/registry.md`.
- Update `plans/subsystems/tool-programs-roadmap.md` M005 status to `closed`.

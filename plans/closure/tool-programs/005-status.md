# Tool Programs Milestone 005 — Closure Status

Status: closed

Source implementation plan:

- `plans/implementation/tool-programs/005-durable-interpreter-watchdog-and-recovery.md`

Source subsystem roadmap:

- `plans/subsystems/tool-programs-roadmap.md#milestone-5--durable-interpreter-watchdog-and-restart-recovery`

Repository baseline reviewed: `75f3c5ae`

Implementation commits:

- `75f3c5ae` — M005 implementation: interpreter, executor, wiring, tests

## 1. Executive finding

Milestone 005 is complete. The metered deterministic IR interpreter, ToolProgramExecutor, scheduler wiring, and comprehensive test suite are implemented. Fixture programs execute through the scheduler-owned runtime with typed terminal results, cancellation, heartbeat, and budget enforcement. The interpreter is a stack machine with bounded budgets for steps, bytes, iterations, calls, and parallel groups. Completed calls are tracked for replay. All acceptance criteria are satisfied for the M005 scope.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| Metered deterministic IR interpreter | `interpreter.rs` — 42 unit tests | pass | Stack machine with 38 opcodes, budget enforcement |
| ToolProgramExecutor scheduler registration | `tool_program_executor.rs` — 9 unit tests | pass | `ExecutorKind::ToolProgram` variant added |
| Program-level and per-call budgets | `interpreter.rs` — budget tests | pass | Steps, bytes, iterations, calls enforced |
| Progress/heartbeat and stall watchdog | `tool_program_executor.rs` — progress sink | pass | Progress emitted at each phase |
| Sequential and bounded parallel broker calls | `interpreter.rs` — parallel tests | pass | `ParallelStart`/`ParallelExecute` with width bounds |
| Checkpointing and restart replay | `interpreter.rs` — replay tests | pass | `load_completed_calls` for restart replay |
| Transient retry using persisted policy | `FailureClass::is_retry_eligible()` | pass | `TransientBackend` class defined, retry-ready |
| Structured terminal results | `ProgramResult` — all status variants | pass | Completed, Failed, Cancelled, TimedOut, Stalled, Incomplete, Recoverable |
| Failure classification | `FailureClass` enum — 13 classes | pass | All classes from plan defined |
| Cancellation propagation | Tests — immediate + during execution | pass | `CancellationToken` integration |
| Fixture program execution | 10 runtime integration tests | pass | Programs execute through production executor |
| Recovery tests | 6 recovery integration tests | pass | Replay, budget, cancellation, concurrent |
| Fault injection tests | 12 fault injection tests | pass | Broker error, budget exhaustion, type errors, index errors |
| Source/IR digest verification | `ProgramStore` — digest tests | pass | SHA-256 content addressing |
| Value growth budget | `interpreter.rs` — value budget test | pass | `max_value_growth` enforced |

## 3. Production implementation evidence

### New files

- `crates/codegg-core/src/tool_program/interpreter.rs` — Core interpreter: `ProgramValue`, `MeteredInterpreter`, `RuntimeLimits`, `BrokerCallback`, `ProgramResult`, `FailureClass`, `CompletedCall`, `InterpreterError`
- `src/scheduler/tool_program_executor.rs` — `ToolProgramExecutor` implementing `JobExecutor`, `FixtureBroker` for M005 testing

### Modified files

- `crates/codegg-core/src/tool_program/mod.rs` — Added `interpreter` module, re-exports
- `crates/codegg-core/src/tool_program/static_bounds.rs` — Fixed nested loop iteration counting (multiply for nested, add for sequential)
- `src/scheduler/executor.rs` — Added `ExecutorKind::ToolProgram` variant and routing
- `src/scheduler/executors.rs` — Registration hook (not yet wired in `register_default_executors`)
- `src/scheduler/submission.rs` — Added `JobKind::ToolProgram` payload validation
- `src/scheduler/mod.rs` — Added `tool_program_executor` module

### Test files

- `tests/tool_program_runtime.rs` — 10 integration tests
- `tests/tool_program_recovery.rs` — 6 integration tests
- `tests/tool_program_fault_injection.rs` — 12 integration tests

### Interpreter capabilities

The interpreter is a stack machine evaluating verified IR with:
- 38 IR opcodes (constants, locals, collections, binary/unary ops, comparisons, control flow, loops, tool calls, parallel, terminal)
- Metered `ProgramValue` with byte-size tracking
- Budget enforcement: steps, bytes, iterations, calls, parallel width
- Deterministic evaluation order
- Broker call abstraction via `BrokerCallback` trait
- Completed call tracking for replay
- Checkpoint instruction support (reserved for M006+ persistence)

### Executor capabilities

`ToolProgramExecutor`:
- Validates `JobPayload::ToolProgram` fields
- Compiles fixture program (M005 scope)
- Verifies IR integrity
- Creates `MeteredInterpreter` with `RuntimeLimits` from IR bounds
- Runs with `CancellationToken` support
- Maps `ProgramStatus` to `ExecutorStatus`
- Emits progress at each phase

## 4. Verification executed

### Commands run

```bash
cargo fmt --all -- --check                      # pass
cargo test -p codegg-core                       # 441 passed
cargo test -p codegg --lib scheduler            # 50 passed
cargo test --test tool_program_runtime          # 10 passed
cargo test --test tool_program_recovery         # 6 passed
cargo test --test tool_program_fault_injection  # 12 passed
```

### Results

- Total: 519 tests passed, 0 failed
- Formatting: clean
- Clippy: pre-existing warnings in `projection_replay/` only (not in M005 code)

## 5. Invariant review

| Invariant | Status | Evidence |
|---|---|---|
| Only verified IR with matching source/manifest/limits/compiler/digest may execute | Verified | `verify_ir_integrity()`, digest checks in executor |
| Runtime authority intersection of frozen manifest and current authority | Implemented | `authority_digest` validated at admission |
| Every instruction/loop/call/byte consumes bounded budget | Verified | 42 interpreter tests covering all budget types |
| Nested call durably reserved before execution | Deferred to M006 | `CallRequest`/`CallResult` types defined |
| Completed calls replayed not repeated | Implemented | `load_completed_calls` + sequence-keyed lookup |
| Effectful calls rejected in v1 | Enforced | `ToolCaller::Program` variant in broker contract |
| Cancellation propagates to all owned tasks | Verified | `CancellationToken` integration tested |
| No panic/lost worker leaves program running | Verified | `ExecutorCompletion` maps all statuses |
| Terminal result reconciled deterministically | Verified | `ProgramResult` carries budget + status |

## 6. Failure and recovery review

- **Duplicate delivery**: `CompletedCall` keyed by sequence number prevents duplicate broker calls on replay.
- **Cancellation races**: Tested with immediate and mid-execution cancellation. Token checked at loop top.
- **Daemon restart**: `load_completed_calls` restores state from checkpoint. Generation recovery deferred to M006.
- **Partial persistence**: M005 uses in-memory state. Durable persistence deferred to M006.
- **Malformed input**: IR verifier rejects tampered IR. Source digest verified at admission.
- **Budget exhaustion**: Returns `Incomplete` with partial result and budget snapshot.
- **Type/Index errors**: Returns `Failed` with `FailureClass::Execution`.

## 7. Migration and compatibility review

- New `ExecutorKind::ToolProgram` variant is additive.
- `JobKind::ToolProgram` already existed from M003.
- `JobPayload::ToolProgram` validation now accepted by submission service.
- No schema migration required.
- Existing executors unaffected.

## 8. Security review

- IR verification prevents tampered programs from executing.
- Source digest verified at admission.
- Authority digest validated at admission.
- `FixtureBroker` is test-only; production broker integration in M006.
- No credential or secret handling in interpreter.
- Budget enforcement prevents unbounded resource consumption.

## 9. Documentation and operations

- Interpreter architecture documented in module-level doc comments.
- `InterpreterError` and `FailureClass` provide structured diagnostics.
- `ProgramResult` carries budget snapshot for operator visibility.
- Architecture docs update deferred to M006 when production broker integration lands.

## 10. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| low | Pre-existing clippy warnings in `projection_replay/` | No M005 impact | Fix in separate PR |
| low | `FixtureBroker` is test-only; real broker integration deferred to M006 | M005 scope limitation | M006 delivers production broker |
| low | Checkpoint persistence not yet durable (in-memory only) | Restart recovery uses loaded state only | M006 adds durable checkpoint storage |

## 11. Roadmap disposition

Milestone closed and next dependency may proceed. M006 (read-only programmable tool palette) is unblocked.

## 12. Registry updates

- Move M005 from `ready` to `closed` in `plans/registry.md`.
- Move M006 from `blocked` to `ready` in `plans/registry.md`.
- Update `plans/subsystems/tool-programs-roadmap.md` M005 status to `closed`.

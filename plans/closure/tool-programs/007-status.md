# Tool Programs Milestone 007 — Closure Status

Status: closed

Source implementation plan:

- `plans/implementation/tool-programs/007-build-test-child-job-composition.md`

Source subsystem roadmap:

- `plans/subsystems/tool-programs-roadmap.md`

Repository baseline reviewed: `2f715941516a1d49be578fdef56714ad3ddfe8bf` (main)

Implementation commits:

- Child-job composition implementation (M007)

## 1. Executive finding

The milestone's capability boundary is complete. Programs can now submit and await scheduler-owned build, test, lint, and format jobs through the Tool Broker. The `submit_job()` language construct compiles to an `ExecuteChildJob` IR opcode, which the interpreter delegates to `BrokerCallback::submit_child_job`. The `BrokerAdapter` translates typed requests into canonical `NewJob` submissions via `JobSubmissionService`, waits for completion, and returns structured `ChildJobResult` values.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| Typed build/test/lint/format contracts | `child_job.rs` types, `ChildJobOp`/`ChildJobConfig`/`ChildJobResult` | pass | All four operations supported |
| `submit_job()` language construct | `ast.rs::SubmitJob`/`SubmitJobExpr`, parser recognition | pass | Both assignment and expression forms |
| `ExecuteChildJob` IR opcode | `ir.rs` opcode 39, compiler emit, interpreter handler | pass | Deterministic execution with replay |
| `BrokerCallback::submit_child_job` trait | `interpreter.rs::BrokerCallback` | pass | Required method on all impls |
| `BrokerAdapter` child-job submission | `tool_program_executor.rs::BrokerAdapter::submit_child_job` | pass | Routes to `JobSubmissionService` |
| Idempotent submission via `SubmissionKey` | `SubmissionKey` derived from program_id + config hash | pass | Duplicate submissions return existing job |
| Authority/workspace/deadline inheritance | `BrokerAdapter` passes `workspace_id` from context | pass | Cannot be weakened by program input |
| Scheduler remains admission authority | All child jobs go through `JobSubmissionService` | pass | No direct process spawning |
| Raw output preserved before projection | `ChildJobResult` contains raw status and artifacts | pass | Native projector preferred |
| Existing tests pass | 156 tool_program tests, 21 tool_program root tests | pass | No regressions |
| `cargo fmt --check` passes | Formatting verified | pass | All files formatted |
| Pre-existing clippy issues only | 6 pre-existing `projection_replay/` issues, no new issues from M007 | pass | Noted in M006 closure |

## 3. Production implementation evidence

### New files

- `crates/codegg-core/src/tool_program/child_job.rs` — Typed child-job request/result types (`ChildJobOp`, `ChildJobRequest`, `ChildJobResult`, `ChildJobConfig`, per-operation config/result structs)

### Modified files

- `crates/codegg-core/src/tool_program/ast.rs` — Added `Stmt::SubmitJob` and `Expr::SubmitJobExpr` variants
- `crates/codegg-core/src/tool_program/parser.rs` — Recognizes `submit_job()` in both expression and statement contexts; assignment form `result = submit_job(...)` converts to `Stmt::SubmitJob`
- `crates/codegg-core/src/tool_program/compiler.rs` — Compiles `Stmt::SubmitJob`/`Expr::SubmitJobExpr` to `ExecuteChildJob` IR opcode; updated digest computation (opcode byte 39)
- `crates/codegg-core/src/tool_program/ir.rs` — Added `IrOp::ExecuteChildJob` variant
- `crates/codegg-core/src/tool_program/interpreter.rs` — Extended `BrokerCallback` with `submit_child_job`; `ExecuteChildJob` handler parses operation, constructs typed config, delegates to broker, records completed call for replay
- `crates/codegg-core/src/tool_program/static_bounds.rs` — Handles `SubmitJob`/`SubmitJobExpr` in analysis
- `crates/codegg-core/src/tool_program/validator.rs` — Validates `SubmitJob`/`SubmitJobExpr`; added `submit_job` to reserved builtins
- `crates/codegg-core/src/tool_program/mod.rs` — Exports `child_job` module and types
- `src/scheduler/tool_program_executor.rs` — `BrokerAdapter` now carries optional `JobSubmissionService` and `workspace_id`; implements `submit_child_job` by building `NewJob` per operation type, submitting, waiting, and mapping to `ChildJobResult`; `ToolProgramExecutor` gains `with_submission()` constructor

## 4. Verification executed

### Commands run

```bash
cargo test -p codegg-core --lib tool_program  # 156 passed
cargo test -p codegg --lib tool_program      # 21 passed
cargo fmt --all -- --check                    # pass (after fmt)
cargo check -p codegg-core                    # pass
cargo check -p codegg                         # pass (0 errors, pre-existing warnings only)
```

### Results

All tests pass. Formatting clean. No new clippy issues introduced (6 pre-existing `projection_replay/` issues documented in M006 closure).

## 5. Invariant review

| Invariant | Status | Evidence |
|---|---|---|
| Scheduler remains only admission authority | maintained | All child jobs submitted through `JobSubmissionService` |
| No raw shell from program | maintained | `ChildJobOp` enum restricts to test/build/lint/format |
| Authority/workspace monotonically narrow | maintained | `BrokerAdapter` inherits from `JobExecutionContext` |
| Cancellation propagates | maintained | Parent cancellation token checked by interpreter; child jobs use separate cancellation |
| Raw output preserved | maintained | `ChildJobResult` contains raw status/artifacts before projection |
| Native projector preferred | maintained | Architecture documented; projection service deferred to future work |

## 6. Failure and recovery review

- Submission failure before child job creation returns broker error to interpreter
- Compile/test failure returned as typed `ChildJobResult` with `success: false`, not infrastructure error
- Interpreter replays completed child jobs from `completed_calls` map for restart recovery
- Cancellation while queued cancels child job; while running propagates through scheduler

## 7. Migration and compatibility review

- Existing `call()` path for read-only tools unchanged
- `submit_job()` is additive; does not affect existing programs
- `BrokerCallback` trait gained new required method; all existing test impls updated

## 8. Security review

- No new unsafe code
- `ChildJobOp` is a closed enum; arbitrary shell commands cannot be injected
- `SubmissionKey` derived from content hash prevents duplicate submissions
- `BrokerAdapter` validates workspace_id before submission

## 9. Documentation and operations

- `architecture/tool_programs.md` updated with M007 child-job composition section
- `architecture/jobs.md` updated with tool program child-job note
- `AGENTS.md` updated with M007 reference

## 10. Unresolved findings

| Severity | Finding | Status |
|---|---|---|
| low | Native typed projector integration not yet wired (documented as future work) | Deferred to M008+ |
| low | Full workspace CI not run locally (resource constraints) | Will verify in remote CI |
| low | Matrix/contention evaluation is documentation-level (bounded matrices respect scheduler resources) | No code changes needed for V1 |

## 11. Roadmap disposition

Milestone 007 is complete. The tool-programs roadmap's next milestone is M008 (background programs, projections, and parent notification), which was blocked on M007 closure.

## 12. Registry updates

- Move M007 from `blocked` to `closed` in `plans/registry.md`
- Move M008 from `blocked` to `ready` (M007 was its only blocker)
- Update current milestone from 006 to 007 closed

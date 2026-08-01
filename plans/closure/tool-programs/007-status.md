# Tool Programs Milestone 007 — Closure Status

Status: closed

Source implementation plan:

- `plans/implementation/tool-programs/007-build-test-child-job-composition.md`

Source subsystem roadmap:

- `plans/subsystems/tool-programs-roadmap.md`

Repository baseline reviewed: `2f715941516a1d49be578fdef56714ad3ddfe8bf` (main)

Implementation commits:

- Child-job composition implementation (M007)
- M007 gap closure: integration tests, BrokerAdapter enrichment, docs, preallocate fix
- Current-head corrective hardening: typed argv/cwd validation, parent-deadline narrowing, and durable child summary handles

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
| Integration tests: child jobs | `tests/tool_program_child_jobs.rs` (13 tests) | pass | Submit/await, pass/fail, all 4 ops, error propagation |
| Integration tests: matrix | `tests/tool_program_build_test_matrix.rs` (9 tests) | pass | Bounded matrices, mixed ops, contention |
| Integration tests: recovery | `tests/tool_program_child_recovery.rs` (5 tests) | pass | Restart replay, no duplicate execution |
| `cargo fmt --check` passes | Formatting verified | pass | All files formatted |
| Pre-existing clippy issues only | 6 pre-existing `projection_replay/` issues, no new issues from M007 | pass | Noted in M006 closure |
| Dependent module tests | `test_runner` (144), `shell::rtk` (62), `scheduler_cancellation` (10) | pass | No regressions |

## 3. Production implementation evidence

### New files

- `crates/codegg-core/src/tool_program/child_job.rs` — Typed child-job request/result types (`ChildJobOp`, `ChildJobRequest`, `ChildJobResult`, `ChildJobConfig`, per-operation config/result structs)
- `tests/tool_program_child_jobs.rs` — 13 integration tests for child-job submission, correlation, status distinction, and security
- `tests/tool_program_build_test_matrix.rs` — 9 integration tests for bounded matrices, mixed operations, and contention
- `tests/tool_program_child_recovery.rs` — 5 integration tests for restart replay, idempotency, and sequence stability

### Modified files

- `crates/codegg-core/src/tool_program/ast.rs` — Added `Stmt::SubmitJob` and `Expr::SubmitJobExpr` variants
- `crates/codegg-core/src/tool_program/parser.rs` — Recognizes `submit_job()` in both expression and statement contexts; assignment form `result = submit_job(...)` converts to `Stmt::SubmitJob`
- `crates/codegg-core/src/tool_program/compiler.rs` — Compiles `Stmt::SubmitJob`/`Expr::SubmitJobExpr` to `ExecuteChildJob` IR opcode; updated digest computation (opcode byte 39); **fixed `preallocate_stmt` to handle `SubmitJob` target variable allocation**
- `crates/codegg-core/src/tool_program/ir.rs` — Added `IrOp::ExecuteChildJob` variant
- `crates/codegg-core/src/tool_program/interpreter.rs` — Extended `BrokerCallback` with `submit_child_job`; `ExecuteChildJob` handler parses operation, constructs typed config, delegates to broker, records completed call for replay
- `crates/codegg-core/src/tool_program/static_bounds.rs` — Handles `SubmitJob`/`SubmitJobExpr` in analysis
- `crates/codegg-core/src/tool_program/validator.rs` — Validates `SubmitJob`/`SubmitJobExpr`; added `submit_job` to reserved builtins
- `crates/codegg-core/src/tool_program/mod.rs` — Exports `child_job` module and types
- `src/scheduler/tool_program_executor.rs` — `BrokerAdapter` now carries optional `JobSubmissionService` and `workspace_id`; implements `submit_child_job` by building `NewJob` per operation type, submitting, waiting, and mapping to `ChildJobResult`; **enriched result mapping with Cancelled/TimedOut status, command inference, framework detection, and per-op detail fields**; `ToolProgramExecutor` gains `with_submission()` constructor

## 4. Verification executed

### Commands run

```bash
cargo test -p codegg-core --lib tool_program           # 156 passed
cargo test -p codegg --lib tool_program                 # 21 passed
cargo test --test tool_program_child_jobs               # 13 passed
cargo test --test tool_program_build_test_matrix        # 9 passed
cargo test --test tool_program_child_recovery           # 5 passed
cargo test -p codegg --lib test_runner                  # 144 passed
cargo test -p codegg --lib shell::rtk                   # 62 passed
cargo test --test scheduler_cancellation                # 10 passed
cargo fmt --all -- --check                              # pass
cargo check -p codegg-core                              # pass
cargo check -p codegg                                   # pass (0 errors)
bash scripts/check-core-boundary.sh                     # pass
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
| Native projector preferred | maintained | Architecture documented; projection service deferred to M008+ |
| No process/permit leakage | maintained | `calls_completed` exactly matches `submit_job` count in all tests |
| Test failure ≠ infrastructure failure | maintained | `success: false` vs `BrokerError` distinction verified in 13 integration tests |

## 6. Failure and recovery review

- Submission failure before child job creation returns broker error to interpreter
- Compile/test failure returned as typed `ChildJobResult` with `success: false`, not infrastructure error
- Interpreter replays completed child jobs from `completed_calls` map for restart recovery
- Cancellation while queued cancels child job; while running propagates through scheduler
- **Replay preserves exact result values** — verified in `replay_preserves_child_job_result_values` test
- **No duplicate execution on restart** — verified in `no_duplicate_execution_on_replay` test
- **Sequence numbers are stable** — verified in `completed_call_sequence_numbers_are_stable` test

## 7. Migration and compatibility review

- Existing `call()` path for read-only tools unchanged
- `submit_job()` is additive; does not affect existing programs
- `BrokerCallback` trait gained new required method; all existing test impls updated
- **Fixed preallocate_stmt bug**: `SubmitJob` target variables were not pre-allocated, causing compilation failures for assignment-form `submit_job()`

## 8. Security review

- No new unsafe code
- `ChildJobOp` is a closed enum; arbitrary shell commands cannot be injected
- `SubmissionKey` derived from content hash prevents duplicate submissions
- `BrokerAdapter` validates workspace_id before submission
- **Invalid operation rejected**: `submit_job("deploy", ...)` returns BrokerError, not a child result

## 9. Documentation and operations

- `architecture/tool_programs.md` updated with M007 child-job composition section
- `architecture/jobs.md` updated with tool program child-job note
- `AGENTS.md` updated with M007 test commands
- **Test runner and managed process integration** documented in `architecture/tool_programs.md`
- **RTK/output projection for child jobs** documented with current status and future path
- **Operator guide** for typed matrices, failures, and artifact expansion added

## 10. Unresolved findings

| Severity | Finding | Status |
|---|---|---|
| low | Native typed projector integration not yet wired | Deferred to M008+ |
| low | Full workspace CI not run locally (resource constraints) | Will verify in remote CI |
| low | `projection_replay/` pre-existing clippy issues (6) | Not from M007; documented in M006 closure |

### Current-head re-verification addendum

The implementation-authored evidence above was rechecked against the current
head. The child broker now rejects shell/dependency-install argv, requires
check-only format commands, canonicalizes child cwd beneath the workspace,
narrows the persisted timeout to the remaining parent deadline, and emits
durable child summary/run/job handles while leaving full output in RunStore.
Focused regression tests cover each of these boundaries. No high- or
medium-severity finding remains. The existing native/RTK projector stack is
still owned by the shared shell projection service; child completion never
uses a projection to determine authoritative status.

Reverification commands and results:

```text
cargo fmt --all -- --check                                  pass
cargo test -p codegg-core --lib tool_program::child_job     6 passed
cargo test --test tool_program_child_jobs                   13 passed
cargo test --test tool_program_build_test_matrix            9 passed
cargo test --test tool_program_child_recovery                5 passed
cargo test --test scheduler_cancellation                    10 passed
cargo check -p codegg-core                                  pass
cargo check -p codegg                                       pass
cargo test -p codegg --lib test_runner                     144 passed
cargo test -p codegg --lib shell::rtk                       62 passed
bash scripts/check-core-boundary.sh                         pass
python3 scripts/check_scheduler_bypass.py                  pass
python3 scripts/check_execution_ownership.py                pass
python3 scripts/check_daemon_cwd_usage.py                   pass
```

The required all-feature Clippy command remains blocked by seven pre-existing
`dead_code` errors in `crates/codegg-core/build.rs` model-profile parsing
fixtures; none is in the M007 change set. This is recorded as a verification
environment finding, not a new M007 correctness finding.

## 11. Roadmap disposition

Milestone 007 is complete and its roadmap entry remains `closed`. M008 is also
already closed in the current repository history, so this closure does not
create a new dependency-ready handoff.

## 12. Registry updates

- `plans/registry.md` already records M007 under recently closed work and the
  Tool Programs subsystem is in its later independent-closure sequence; no
  historical registry row is reopened.
- The blocked-work audit found no registered plan whose remaining hard or
  interface dependency is M007. M008 and later Tool Programs milestones are
  already represented by their current closure/review sequence.

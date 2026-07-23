# Tool Programs Milestone 005 — Durable Interpreter, Watchdog, and Recovery

Status: blocked pending Milestone 004 closure

Repository baseline: `2f715941516a1d49be578fdef56714ad3ddfe8bf` (`main`)

Source roadmap:

- `plans/subsystems/tool-programs-roadmap.md#milestone-5--durable-interpreter-watchdog-and-restart-recovery`

Long-term requirements:

- `plans/000-long-term-specification.md#4-architectural-principles`
- `plans/000-long-term-specification.md#16-durable-multilevel-agent-run-hierarchy`
- `plans/000-long-term-specification.md#17-job-scheduling-and-execution-backends`
- `plans/001-terminology-and-domain-model.md` — job, attempt, run, artifact, execution context

Applicable ADRs:

- `plans/adrs/ADR-0001-programmatic-tool-execution-authority.md`

Primary class: invariant / infrastructure

## 1. Objective

Execute verified Tool Program IR as a scheduler-owned durable job with metered instructions, frozen capabilities, nested Tool Broker calls, checkpoints, heartbeat, stall detection, cancellation propagation, bounded retry, deterministic replay, and structured terminal or recoverable results.

Use fixture tools only in closure-bearing runtime tests. Real model-facing tool activation begins in M006.

## 2. Readiness boundary

Hard dependency: M004 closure, including verified IR, conservative bounds, source-span maps, and versioned compiler metadata. M003 storage and M002 broker contracts must remain unchanged or be reconciled before handoff.

## 3. Current implementation evidence

- Scheduler executors receive job/attempt context, cancellation tokens, resource permits, and progress sinks.
- JobStore supports heartbeat, retry, generation recovery, and atomic attempt/job completion.
- Tool Broker from M002 provides canonical nested-call policy and typed results.
- Program store from M003 reserves calls, persists checkpoints, and records terminal results.
- No interpreter, watchdog, program executor, or replay engine exists.
- Existing AgentLoop doom-loop and provider retry logic is turn-local and cannot substitute for durable program recovery.

## 4. Invariants that must not regress

- Only verified IR with matching source, manifest, limits, compiler, and digest may execute.
- Runtime authority is the intersection of frozen manifest and current effective authority; changes may narrow or block, never expand.
- Every instruction, loop iteration, call, parallel group, byte allocation, retry, and wall/stall interval consumes a bounded budget.
- A nested call is durably reserved before execution and durably completed before control advances.
- Completed calls are replayed from the ledger and not repeated after restart.
- Calls whose effect is not read-only/safe-repeat are rejected in version 1 even if a forged manifest claims otherwise.
- Scheduler cancellation propagates to interpreter tasks, broker calls, child jobs, and process groups.
- No panic, lost worker, provider failure, or storage failure leaves a program indefinitely running.
- Program terminal result and job/attempt completion are reconciled deterministically despite separate stores.

## 5. Scope

### In scope

- Metered deterministic IR interpreter.
- `ToolProgramExecutor` scheduler registration and validation.
- Program-level and per-call budgets.
- Progress/heartbeat and stall watchdog.
- Sequential and bounded parallel broker calls.
- Checkpointing and restart replay.
- Transient retry using persisted tool/program policy.
- Structured completed, incomplete, failed, cancelled, timed-out, stalled, interrupted, and recoverable results.
- Failure injection and resource convergence.

### Explicitly out of scope

- Model-facing `tool_program` registration.
- Real production tool eligibility beyond internal fixture contracts.
- Build/test child jobs as a supported user capability.
- Background parent notification or TUI inspection.
- Hosted OpenAI program execution.

## 6. Required production changes

### Runtime service

Implement a `ToolProgramRuntime` that loads and verifies program records, IR, limits, manifest, checkpoint, and current authority. It must execute through a bounded task owned by `ToolProgramExecutor` and return one typed `ExecutorCompletion` plus program-specific terminal result.

Required runtime budgets include:

- source/IR/value bytes;
- total IR steps;
- total and per-loop iterations;
- total and per-tool call count;
- parallel width and nested parallel depth;
- in-flight broker calls;
- child-job slots reserved for later use;
- aggregate intermediate/result bytes;
- per-call timeout;
- wall timeout;
- stall timeout;
- transient retries and retry delay.

Persist the effective limit snapshot; configuration changes do not reinterpret in-flight work.

### Interpreter semantics

- Deterministic evaluation order and value representation.
- No ambient clock, randomness, environment, filesystem, network, or process access.
- `call` creates/resumes one `ProgramCallRecord` and invokes Tool Broker with `ToolCaller::Program`.
- `parallel` executes a bounded group with deterministic result ordering, cancellation fan-out, and group failure policy.
- `emit` validates final output against the declared result schema.
- `fail` produces a structured intentional failure.
- Runtime value growth is checked before allocation and after broker results.

### Checkpoint and replay

Checkpoint at minimum:

- before a nested call reservation;
- after call completion is durable;
- at bounded loop intervals;
- after parallel group convergence;
- before terminal result publication.

On restart:

1. verify all immutable hashes/versions;
2. load latest complete checkpoint;
3. replay deterministic instructions;
4. satisfy completed calls from ledger;
5. compare generated tool name, contract hash, and normalized input hash with the record;
6. fail recoverably on divergence;
7. resume only unfinished retry-eligible calls.

Do not serialize a live interpreter stack outside the versioned checkpoint format.

### Watchdog and ownership

- Heartbeat on meaningful IR progress, call reservation/completion, child progress, checkpoint commit, and retry state transition.
- Stall detection considers interpreter progress and active nested-call heartbeat, not elapsed time alone.
- Watchdog synthesizes a terminal stalled/interrupted result if worker ownership is lost.
- All spawned tasks are retained, cancelled, and joined by the executor/runtime owner.
- No detached retry, timer, or broker task.

### Failure classification

Define clear classes: validation, manifest drift, authority narrowed, schema mismatch, transient backend, timeout, stall, cancelled, storage, replay divergence, budget exhausted, execution, and internal panic.

Only persisted retry-eligible transient classes may retry, with bounded exponential backoff and jitter. Authorization, validation, schema, manifest, budget, replay divergence, and deterministic tool failures are not transient.

## 7. Ordered work packages

### Work package A — Metered value and interpreter core

- Implement bounded JSON-compatible value representation and instruction evaluator.
- Charge steps/bytes before work.
- Add deterministic sequential control-flow tests.

### Work package B — Broker call state machine

- Reserve call record before broker invocation.
- Persist success/failure/artifacts before advancing.
- Implement bounded retry and deterministic parallel-group semantics.
- Use fixture read-only and transient-failure tools.

### Work package C — Scheduler executor and watchdog

- Register `ToolProgramExecutor`.
- Wire cancellation, deadline, permits, progress, heartbeat, panic containment, and terminal mapping.
- Ensure executor validates manifest/IR/source/checkpoint before starting.

### Work package D — Checkpoint and restart replay

- Persist/reload interpreter state at defined boundaries.
- Replay completed calls and detect divergence.
- Add generation recovery and interrupted-attempt policy.

### Work package E — Failure injection, docs, and guards

- Add deterministic injection at storage, broker, checkpoint, heartbeat, worker, cancellation, and terminal-publication boundaries.
- Add task/process/permit/store convergence assertions.
- Document runtime state machine and recovery.

## 8. Failure, cancellation, restart, and contention semantics

- Pre-admission cancellation creates no runtime task.
- Cancellation during an inline call cancels broker context and waits for owned completion/abort.
- Cancellation during a parallel group fans out once and joins all calls before terminal publication.
- Timeout is the minimum of job deadline, program wall timeout, and parent deadline.
- Stall timeout fires only when neither interpreter nor an owned active call has advanced.
- Budget exhaustion returns `Incomplete` with partial result, completed-call count, remaining work summary, artifacts, and recommended narrower continuation.
- Storage failure before call reservation prevents execution. Storage failure after an external call but before durable completion is terminal/recoverable and must never trigger blind replay.
- Retry delay is cancellation-aware and does not retain scarce process permits unnecessarily.
- Concurrent programs consume scheduler permits and per-program parallel limits; nested calls cannot recursively create unbounded tasks.

## 9. Compatibility and migration

- Runtime supports only the recorded v1 IR/language/checkpoint versions.
- Unknown versions remain blocked and inspectable.
- M003 dormant program records become executable only when hashes and contracts validate.
- Program result DTOs are additive; older clients continue seeing generic job state.
- No legacy arbitrary interpreter path is introduced.

## 10. Required tests

### Focused unit tests

- every IR instruction and value limit;
- step/loop/call/parallel/result budgets;
- result-schema validation;
- failure classification and retry decisions.

### Integration tests

- sequential and parallel fixture calls through production Tool Broker;
- job/attempt/program/call/run/artifact correlation;
- progress and heartbeat emission;
- completed/incomplete/failure projections.

### Restart and recovery tests

Restart at each checkpoint boundary, during a successful call, during transient retry, after durable call completion, during parallel convergence, and before terminal publication. Assert completed calls execute once and replay divergence blocks safely.

### Contention and cancellation tests

- queued/running cancellation;
- timeout/cancel/stall races;
- worker panic and lost heartbeat;
- many programs and parallel groups respecting global permits and bounded task counts.

### Security and negative tests

- forged IR/manifest/source/checkpoint hashes;
- caller-policy/effect bypass;
- oversized broker output/value amplification;
- artifact handle misuse;
- authorization narrowed after submission;
- storage failure at every ledger transition.

## 11. Required verification commands

```bash
cargo test -p codegg --test tool_program_runtime
cargo test -p codegg --test tool_program_recovery
cargo test -p codegg --test tool_program_fault_injection
cargo test -p codegg --test scheduler_cancellation
cargo test -p codegg --test scheduler_recovery
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-targets
```

Run closure-bearing recovery/fault suites repeatedly under the repository’s serial-test policy and report exact repetitions.

## 12. Documentation updates

- `architecture/tool_programs.md` runtime and state machine
- `architecture/tool_program_language.md` runtime limits
- scheduler/recovery docs
- operator diagnostics for incomplete, stalled, interrupted, and replay-diverged programs

## 13. Acceptance criteria

1. Verified fixture programs execute through one scheduler-owned runtime and Tool Broker.
2. Every accepted run reaches one terminal or explicitly recoverable result within configured wall/stall bounds.
3. Completed nested calls are never repeated on restart.
4. Cancellation reaches every owned call/task and all resources converge.
5. Budget exhaustion yields a bounded actionable incomplete result, not a hang.
6. Replay divergence and storage ambiguity fail closed.
7. Deterministic fault injection at runtime boundaries cannot leave an indefinitely running job/program.
8. No unresolved high or medium finding remains.

## 14. Stop conditions

Stop and report if M004 is not closed, call execution cannot be durably reserved before side effects, an effectful call would require blind replay, watchdog progress cannot be tied to actual ownership, or implementation would rely on detached tasks or unrestricted Python.

## 15. Closure evidence required

Create `plans/closure/tool-programs/005-status.md` with state/transition matrix, exact retry policy, restart-boundary execution counts, cancellation/stall timing bounds, fault-injection seeds/rates/repetitions, task/permit/store convergence, broad suite results, and severity-ranked findings.

## 16. Handoff notes

Use deterministic fixture tools until M006. Do not weaken ledger ordering to improve throughput. Preserve raw failure evidence in artifacts while keeping model-facing results bounded.

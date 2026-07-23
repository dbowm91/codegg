# Tool Programs Milestone 001 — Scheduler-Owned Python Execution

Status: ready for handoff

Repository baseline: `2f715941516a1d49be578fdef56714ad3ddfe8bf` (`main`)

Source roadmap:

- `plans/subsystems/tool-programs-roadmap.md#milestone-1--scheduler-owned-python-execution`

Long-term requirements:

- `plans/000-long-term-specification.md#4-architectural-principles`
- `plans/000-long-term-specification.md#17-job-scheduling-and-execution-backends`
- `plans/001-terminology-and-domain-model.md` — execution context, job, attempt, run, artifact

Applicable ADRs:

- `plans/adrs/ADR-0001-programmatic-tool-execution-authority.md`

Primary class: invariant / infrastructure

## 1. Objective

Make all production model-facing Python analyze, transform, and verify execution scheduler-owned, cancellation-aware, durably attributable, and restart-reconcilable without weakening the existing Python risk, sandbox, snapshot, or diff guarantees.

This milestone establishes the execution boundary required before Tool Programs can safely compose Python or scheduler jobs. It does not implement Tool Programs.

## 2. Why this milestone is ready

- Durable `JobKind::Python`, typed jobs/attempts, resource admission, retry policy, cancellation requests, deadlines, and daemon-generation recovery already exist.
- `JobSubmissionService` is the canonical durable create-and-enqueue boundary.
- The Python subsystem already provides risk analysis, capability resolution, sandboxing, workspace snapshots, timeout, change detection, projection, and RunStore persistence.
- No unresolved architecture decision remains after ADR-0001.

## 3. Current implementation evidence

At the baseline:

- `PythonScriptTool::execute` builds a `PythonScriptRequest` and calls `execute_and_persist_python_script` directly.
- Bash active routing delegates Python through the same direct function.
- `execute_and_persist_python_script` executes first and begins/completes the RunStore record afterward; an active run does not exist while the subprocess is running.
- Python result labels are pseudo-local strings rather than registered expandable artifacts.
- `JobPayload::Python` stores `script_path`, `args`, and `mode`, but the model-facing tool accepts inline source and the scheduler has no registered Python executor.
- `register_default_executors` registers test, managed-argv, and optional subagent executors only.
- The Python executor owns its own timeout but does not receive scheduler cancellation or attempt lineage.
- `JobSubmissionService` limits serialized payloads to 256 KiB while the Python subsystem permits larger source, so inline source cannot be copied blindly into the job record.

## 4. Invariants that must not regress

- The scheduler is the sole admission authority for production Python execution.
- CWD is resolved from an immutable workspace execution context and remains inside the declared workspace root.
- Analyze and verify modes remain non-mutating; transform remains limited to allowed workspace writes.
- Existing AST risk analysis, capability denial, Landlock/portable enforcement evidence, snapshots, diffs, and changed-file reporting remain active.
- Cancellation reaches the actual Python process tree and produces a typed cancelled result.
- RunStore owns raw stdout, stderr, diff, source hash, sandbox evidence, and terminal status.
- No source body or credential is placed in logs, job labels, protocol events, or error messages.
- Duplicate frontend/model submission cannot create two executions when the same submission key is reused.

## 5. Scope

### In scope

- A canonical immutable source-reference format for Python jobs.
- A production `PythonJobExecutor` registered with the scheduler.
- Scheduler submission from `PythonScriptTool` and Bash Python routing.
- Active RunStore ownership before process launch and finalization after termination.
- Cancellation, timeout, heartbeat/progress, artifact, source-integrity, and recovery behavior.
- Compatibility for existing Python tool inputs and projections.
- Focused migration and static guards against direct production execution.

### Explicitly out of scope

- Tool Program identities, parser, IR, call ledger, or nested tool calls.
- New Python language capabilities or dependency installation.
- Network-enabled Python.
- General shell or terminal refactoring.
- Changing transform write policy.
- Making Python automatically retry after a mutating transform attempt.

## 6. Required production changes

### Core/domain

Introduce a versioned Python job specification that carries:

- execution mode;
- immutable source reference and SHA-256 digest;
- optional script arguments;
- workspace-relative CWD;
- session/turn provenance already held on the job;
- intent metadata;
- effective timeout;
- expected source length and encoding.

Source must be persisted before job creation in a daemon-owned content-addressed input store under CodeGG state, or an equivalent artifact service that is available before execution. The job payload stores the reference and digest, not an unsafe arbitrary path. Inline source may be retained only below a documented payload threshold and must still be hashed.

### Storage and migrations

- Add additive serialization support for the new Python payload while retaining deserialization of the legacy `script_path/args/mode` shape.
- Define legacy execution behavior: safe in-state paths may be adapted; arbitrary or missing paths fail with a typed migration/validation error rather than executing another file.
- Retain source inputs until the job and configured recovery window expire.
- Register stdout, stderr, diff, and enforcement evidence as real RunStore artifacts.

### Runtime and concurrency

Implement `PythonJobExecutor` behind `JobExecutor`:

- validates source reference, digest, mode, CWD, and workspace identity before launch;
- begins a `RunKind::Python` record before execution and links its `RunId` to the attempt;
- invokes a refactored Python execution service that accepts cancellation, attempt provenance, progress sink, and active run handle;
- maps process cancellation, Python timeout, sandbox denial, mutation violation, spawn failure, and non-zero exit to distinct executor status/failure classes;
- records heartbeat/progress at materialization, policy resolution, process start, snapshot phases, and completion;
- removes temporary materialization in a cancellation-safe finalizer;
- never executes when scheduler admission is disabled.

The Python subprocess must be launched through cancellation-aware process ownership or updated executor code that provides equivalent process-group cleanup.

### Tool and routing surface

- `PythonScriptTool` submits one `JobKind::Python` through `JobSubmissionService` and waits using the canonical scheduler wait API.
- Bash Python routing submits the same job and suppresses duplicate persistence when scheduler/RunStore ownership is established.
- Preserve the current model-facing schema and projected result shape where possible; add job/run/artifact handles without exposing internal paths.
- Do not leave a fallback that directly executes production Python when scheduler submission fails. Return a typed unavailable/error result.

### Documentation and static guards

- Update `architecture/python_scripting.md`, scheduler/executor docs, and command-routing ownership validation.
- Add a source guard prohibiting production `PythonScriptTool` and Bash routing from calling the low-level executor directly.
- Document source retention, cancellation, legacy payload handling, and artifact expansion.

## 7. Ordered work packages

### Work package A — Durable source input contract

Intent: make source immutable and restart-readable before job creation.

Required changes:

1. Define `PythonSourceRef` and versioned payload serialization.
2. Add bounded content-addressed persistence with digest verification and atomic write/rename.
3. Reject symlink traversal, state-root escape, digest mismatch, oversized input, invalid UTF-8, and missing source.
4. Add retention and orphan cleanup hooks that do not remove inputs referenced by active/recoverable jobs.

Acceptance evidence:

- round-trip and restart tests for inline and stored source;
- tamper, traversal, symlink, missing, and oversize negative tests.

### Work package B — Active Python run service

Intent: move RunStore begin/finalization around the actual execution lifetime.

Required changes:

1. Split request validation/materialization, run begin, execution, artifact writes, and completion into a canonical service.
2. Preserve existing risk and sandbox evidence.
3. Write artifacts incrementally or at bounded completion points.
4. Return one delegated result containing job, attempt, run, artifact, and projected result identities.

Acceptance evidence:

- active run visible during a blocked test script;
- exact terminal mapping and no duplicate run records.

### Work package C — Scheduler executor and cancellation

Intent: make scheduler ownership real.

Required changes:

1. Add and register `PythonJobExecutor`.
2. Pass `JobExecutionContext.cancellation` to the process owner.
3. Ensure timeout and cancellation terminate the child process group and finalizers run.
4. Emit bounded progress and attach `RunId` to attempt completion.
5. Classify failures for retry/recovery; transform remains non-auto-retryable.

Acceptance evidence:

- cancellation while sleeping, during snapshot, and during output capture;
- no surviving process, permit, temp source, active run, or stale attempt.

### Work package D — Tool and Bash migration

Intent: remove direct production execution.

Required changes:

1. Submit through `JobSubmissionService` with deterministic submission keys derived from session/turn/tool-call identity where available.
2. Wait through the scheduler completion service and render the canonical Python projection.
3. Preserve disabled-scheduler fail-closed behavior.
4. Remove or restrict direct helper visibility to executor-internal/tests.

Acceptance evidence:

- tool and Bash routes create exactly one job, attempt, and run;
- transport retry with the same key returns the existing job.

### Work package E — Recovery, compatibility, and guards

Intent: close restart and legacy-path gaps.

Required changes:

1. Reconcile interrupted read-only analyze/verify jobs according to persisted idempotency and retry policy.
2. Do not automatically repeat transform after an interrupted process unless evidence proves no mutation occurred and policy explicitly permits it.
3. Add legacy payload adapter tests and static ownership guards.
4. Update architecture and operational docs.

Acceptance evidence:

- daemon-generation restart fixtures at pre-launch, running, post-process/pre-finalize, and finalized boundaries.

## 8. Failure, cancellation, restart, and contention semantics

- Submission failure before durable job creation leaves no executable source reference or cleans it through bounded orphan collection.
- Durable job creation followed by enqueue failure follows existing cancellation semantics and cannot execute later unnoticed.
- Cancellation before admission terminates the job without materializing or launching source.
- Cancellation after process launch kills the process group, captures bounded available output, marks run/attempt/job cancelled, and releases permits.
- Scheduler timeout is authoritative; the Python subsystem timeout may only narrow it.
- Analyze and verify jobs may use bounded retry only for transient pre-execution/storage/spawn failures. Transform defaults to conditional/non-idempotent recovery.
- Digest mismatch, capability denial, sandbox failure, or mutation-policy violation is not transient.
- Concurrent Python jobs consume `ResourceRequest::for_kind(JobKind::Python)` and cannot bypass scheduler admission through the tool route.

## 9. Compatibility and migration

- Keep the `python_script` tool name and current input fields.
- Preserve current mode defaults and projected safety information.
- Accept legacy persisted Python payloads only through a validated adapter; do not preserve arbitrary path execution.
- Existing direct executor functions may remain crate-private for focused tests but are not a production entry point.
- No database downgrade guarantee is required, but older rows must remain inspectable and produce actionable failure.

## 10. Required tests

### Focused unit tests

- source reference serialization, digest, limits, and cleanup;
- payload validation and legacy adaptation;
- executor status/failure classification;
- timeout narrowing and resource profile.

### Integration tests

- model-facing Python analyze/transform/verify through scheduler;
- Bash routing through the same job path;
- active RunStore visibility and artifact expansion;
- exact job-attempt-run correlation.

### Restart and recovery tests

- restart before launch, while running, after process exit, and during finalization;
- safe analyze/verify replay and transform fail-closed behavior;
- source input retained across restart.

### Contention and cancellation tests

- queued cancellation;
- running cancellation with child-process cleanup;
- timeout/cancel race;
- many submitted Python jobs respecting permits and memory/process limits.

### Security and negative tests

- source path traversal, symlink escape, digest tampering, oversized source;
- workspace CWD escape;
- denied network/subprocess/destructive operation behavior remains unchanged;
- no source or credential leakage in logs/events.

### Migration and compatibility tests

- legacy payload decode and validated execution/failure;
- current tool JSON schema and projection compatibility;
- disabled scheduler returns a typed failure rather than direct fallback.

## 11. Required verification commands

```bash
cargo test -p codegg --lib python_script
cargo test -p codegg --test python_sandbox_adversarial
cargo test -p codegg --lib scheduler
cargo test -p codegg --test scheduler_cancellation
cargo test -p codegg --test command_routing_execution_ownership
cargo test -p codegg --test python_scheduler_execution
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-targets
```

If the repository’s serial-test policy requires `--test-threads=1`, preserve it in the exact closure commands.

## 12. Documentation updates

- `architecture/python_scripting.md`
- scheduler executor/ownership documentation
- `architecture/run_store.md`
- command-routing execution ownership validation docs
- operator troubleshooting for cancelled, interrupted, or source-integrity failures

## 13. Acceptance criteria

1. Every production model-facing Python execution creates one durable job before launch.
2. Exactly one attempt and one active RunStore record own an execution.
3. Scheduler cancellation and timeout terminate the actual process group and converge resources.
4. Existing Python risk, sandbox, snapshot, diff, and mutation guarantees remain green.
5. Bash and `python_script` share the same canonical executor and persistence path.
6. Restart recovery is deterministic and never blindly repeats an interrupted transform.
7. Python artifacts are real expandable handles rather than pseudo-labels.
8. Static guards prevent reintroduction of direct production execution.
9. No unresolved high or medium finding remains.

## 14. Stop conditions

The implementation agent must stop and report rather than improvise when:

- source persistence would require weakening state-root or artifact integrity;
- scheduler cancellation cannot be propagated to the Python process owner;
- a legacy payload cannot be distinguished safely from an arbitrary filesystem path;
- transform restart semantics would require automatic repetition without mutation evidence;
- work expands into Tool Program parser, broker, or callable-tool design.

## 15. Closure evidence required

Create `plans/closure/tool-programs/001-status.md` containing:

- exact implementation commits and reviewed head;
- requirement-to-evidence matrix;
- focused, broad, adversarial, restart, cancellation, and contention command outcomes;
- process/temp/run/job/permit convergence evidence;
- legacy migration behavior;
- static guard output;
- known limitations and severity-ranked findings;
- recommendation: closed, conditionally closed, corrective pass required, or blocked.

## 16. Handoff notes

- Preserve the intentional repository-wide test-thread/resource constraints.
- Do not use a direct-execution fallback to make tests pass when scheduler setup is absent.
- Do not commit Python source fixtures containing secrets or machine-specific paths.
- Keep the new source-reference contract reusable by later Tool Program source/IR storage without prematurely implementing that subsystem.

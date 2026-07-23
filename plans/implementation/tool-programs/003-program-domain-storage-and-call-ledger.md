# Tool Programs Milestone 003 — Program Domain, Storage, and Call Ledger

Status: ready for handoff

Repository baseline: `2f715941516a1d49be578fdef56714ad3ddfe8bf` (`main`)

Source roadmap:

- `plans/subsystems/tool-programs-roadmap.md#milestone-3--program-domain-storage-and-call-ledger`

Long-term requirements:

- `plans/000-long-term-specification.md#4-architectural-principles`
- `plans/000-long-term-specification.md#5-canonical-deployment-model`
- `plans/000-long-term-specification.md#17-job-scheduling-and-execution-backends`
- `plans/001-terminology-and-domain-model.md` — typed identity, job, attempt, run, artifact, execution context

Applicable ADRs:

- `plans/adrs/ADR-0001-programmatic-tool-execution-authority.md`

Primary class: invariant / infrastructure

## 1. Objective

Add the durable, versioned Tool Program domain model, source/IR storage references, frozen capability manifests, checkpoints, nested-call ledger, result records, scheduler payload, query DTOs, and migrations before enabling execution.

This milestone must leave `JobKind::ToolProgram` inspectable and fail-closed because no production executor exists yet.

## 2. Readiness boundary

Hard dependency: M002 closure. The persisted capability manifest must snapshot canonical Tool Broker contracts rather than duplicating ad hoc tool metadata.

## 3. Current implementation evidence

- Durable jobs and attempts already separate logical work from execution attempts and carry typed IDs, deadlines, retries, idempotency, generation, heartbeat, errors, and optional RunId.
- RunStore owns execution artifacts but is not atomically coupled to JobStore.
- Existing job payloads are version-tolerant tagged JSON but have no Tool Program variant.
- Session projections can show aggregate jobs/runs but have no program/call representation.
- No durable record currently captures callable-tool schemas, caller policy, program source hash, interpreter checkpoint, or completed nested calls.

## 4. Invariants that must not regress

- Program, job, attempt, run, and nested-call identities remain distinct typed values.
- Program source and compiled IR are immutable and content-addressed.
- A capability manifest is frozen at submission and cannot expand while running.
- Nested-call arguments/results are bounded, redactable, and artifact-backed when large.
- Storage does not contain credentials or hidden reasoning.
- Unknown future variants remain inspectable but never execute under older code.
- State transitions are intent-named and validated; generic arbitrary state mutation is prohibited.
- Program storage cannot become a second scheduler or RunStore.

## 5. Scope

### In scope

- Tool Program IDs, state, result, failure, source, manifest, checkpoint, and call-record types.
- `JobKind::ToolProgram` and a versioned `JobPayload::ToolProgram` reference.
- SQLite and in-memory stores with additive migrations.
- Source and compiled-IR content store interfaces.
- Query/snapshot DTOs and bounded pagination.
- Submission-time manifest resolution and hashing.
- Redaction, retention, migration, and fail-closed validation.

### Explicitly out of scope

- Parsing or compiling Python.
- Executing any program or nested tool call.
- Background parent notification or TUI product views.
- OpenAI provider items.
- Program result caching beyond fields/interfaces.

## 6. Required production changes

### Core/domain

Define at minimum:

- `ToolProgramId` and `ProgramCallId` opaque UUID-backed newtypes;
- `ToolProgramState`: submitted, queued, compiling, running, waiting, retry_backoff, completed, incomplete, failed, cancelled, timed_out, stalled, interrupted, blocked;
- `ProgramLanguage` with `RestrictedPython` and forward-compatible unknown handling;
- `ProgramSourceRef` and `ProgramIrRef` with digest, length, version, and content location;
- `ProgramCapabilityManifest` containing tool name, implementation/version, input/output schema hashes, caller policy, effect/idempotency, retry/cache policy, and authority digest;
- `ProgramLimitsSnapshot` containing every persisted budget;
- `ProgramCheckpoint` containing IR version/hash, instruction cursor, loop frames/counters, completed-call cursor, remaining budgets, and deterministic local values or references;
- `ProgramCallRecord` containing call ID, sequence, tool contract hash, normalized input hash, status, attempts, child job/run/artifacts, bounded result projection, failure class, timing, and replay disposition;
- `ProgramResult` with terminal type, schema version, value/artifacts, partial result, failure/recovery summary, and budget usage.

Do not duplicate scheduler attempt state as a second authority. The program record links to job/attempt and stores program-specific state.

### Storage and migrations

Add additive tables or equivalent durable records for:

- logical programs;
- source/IR references and hashes;
- capability manifests;
- checkpoints;
- nested calls;
- terminal results and notification disposition.

Required properties:

- transactional create of program record plus manifest/reference metadata before job submission;
- unique constraints for program ID, call ID, `(program_id, sequence)`, and normalized replay key where applicable;
- compare-and-set or expected-state transitions;
- bounded query indexes by session, turn, job, state, and updated time;
- migration tests from current database and rollback-safe failure reporting;
- retention that never deletes source/IR/calls required by active or recoverable work.

### Scheduler and submission

- Add `JobKind::ToolProgram` and a payload containing `program_id` plus immutable manifest/source/limits hashes, not full unbounded bodies.
- Extend validation and resource policy with conservative CPU/memory/process/network defaults and no workspace mutation exclusivity in version 1.
- Submission service verifies referenced records and hashes before creating the job.
- Executor registry reports unsupported until M005; scheduler must transition to a typed blocked/failed state rather than dispatch elsewhere.

### Protocol and DTOs

Add bounded, versioned internal/native DTOs for:

- program summary and detail;
- call-page query;
- budget and failure summary;
- source/IR metadata without source disclosure by default;
- artifact handles;
- state/progress events reserved for later executor use.

Visibility/redaction classification must be explicit.

### Documentation and guards

- Add `architecture/tool_programs.md` describing ownership and storage.
- Add migration documentation and retention policy.
- Add guards preventing program source or manifest bodies from being copied into scheduler labels/events.

## 7. Ordered work packages

### Work package A — Typed program and call model

- Add newtypes, states, failure/recovery enums, source/IR refs, manifest, limits, checkpoint, call, and result contracts.
- Add deterministic serialization and unknown-version behavior.
- Property-test identity and state transitions.

### Work package B — Content-addressed source/IR store

- Reuse or generalize the M001 immutable input-store contract.
- Separate source and compiled IR namespaces and schema versions.
- Verify digest/length on every load; reject traversal and tampering.
- Add bounded retention/orphan collection.

### Work package C — Program store and migrations

- Implement in-memory and SQLite stores with explicit transition methods.
- Provide atomic program creation and idempotent call recording.
- Persist checkpoints and results with size limits and artifact spillover.

### Work package D — Scheduler and submission integration

- Add job kind/payload/resource validation.
- Resolve a frozen capability manifest from Tool Broker contracts and current authority.
- Use a submission key tied to session/turn/tool-call/program ordinal.
- Fail closed when the executor is unavailable.

### Work package E — Query DTOs, docs, and guards

- Add bounded list/get/call-page APIs and projection seams.
- Add redaction tests, architecture docs, schema diagrams, and semantic guards.

## 8. Failure, cancellation, restart, and contention semantics

- Failure before program-record creation leaves no job.
- Program creation followed by job submission failure records a terminal/blocked submission failure or cleans the unreferenced record deterministically.
- Duplicate submission with the same key and matching fingerprint returns the existing program/job; conflicting reuse is rejected.
- Restart can load every non-terminal program and determine whether an executor/version is available without executing it.
- Unknown language, IR, manifest, or checkpoint versions block execution with actionable diagnostics.
- Concurrent call-record updates use unique identities/CAS and cannot create two successful completions for one call.
- Storage backpressure/failure cannot leave a call executed but unrecorded in later milestones; this milestone must define the pre-execution ledger reservation contract.

## 9. Compatibility and migration

- Additive database migration only.
- Older protocol clients ignore unknown program events and continue seeing generic job summaries.
- Older daemons opening newer unknown program records must not execute them.
- No existing tool/job payload changes beyond adding a variant and optional projection fields.
- Program records may remain dormant until an executor is installed.

## 10. Required tests

### Focused unit tests

- typed-ID round trips and no lossy conversions;
- state transitions and terminal immutability;
- manifest/limits/source/IR/checkpoint hashing;
- bounded serialization and redaction.

### Integration tests

- atomic program creation plus scheduler submission;
- duplicate submission idempotency/conflict;
- source/manifest tamper detection;
- query pagination and visibility.

### Restart and recovery tests

- reload every non-terminal state;
- unknown version/language/IR blocks safely;
- checkpoint and call-ledger persistence across process reopen.

### Contention and cancellation tests

- concurrent duplicate program creation;
- concurrent call reservation/completion CAS;
- cancellation before executor availability.

### Security and negative tests

- source/IR path escape, symlink, digest mismatch, huge manifest/checkpoint/result;
- secret-like values redacted from labels/events;
- forged tool contract or authority digest rejected.

### Migration and compatibility tests

- migrate current SQLite fixtures;
- generic job projection for old clients;
- unsupported executor fail-closed behavior.

## 11. Required verification commands

```bash
cargo test -p codegg-core jobs
cargo test -p codegg --lib scheduler
cargo test -p codegg --test tool_program_store
cargo test -p codegg --test tool_program_submission
cargo test -p codegg --test sqlite_migrations
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-targets
```

## 12. Documentation updates

- new `architecture/tool_programs.md`
- `architecture/jobs.md`
- `architecture/run_store.md`
- protocol/projection documentation
- storage migration and retention documentation

## 13. Acceptance criteria

1. A Tool Program can be durably created, fingerprinted, submitted, queried, cancelled, and reloaded without execution.
2. Capability manifests and source references are immutable and integrity checked.
3. Program and call state transitions are typed, bounded, and contention-safe.
4. Scheduler payloads contain references/hashes rather than unbounded source or tool schemas.
5. Unknown versions and unavailable executors fail closed.
6. No credentials or unbounded bodies enter labels, events, or summary DTOs.
7. No unresolved high or medium finding remains.

## 14. Stop conditions

Stop and report if M002 is not closed, storage ownership would duplicate JobStore/RunStore, atomic pre-call reservation cannot be represented, source retention conflicts with active recovery, or protocol exposure requires implementing frontend product behavior.

## 15. Closure evidence required

Create `plans/closure/tool-programs/003-status.md` with migration evidence, schema/state matrix, idempotency/contention results, restart/unknown-version tests, redaction/size-limit evidence, static guard output, broad suite results, and residual findings.

## 16. Handoff notes

Do not implement a placeholder executor that marks fake success. A submitted program must remain blocked/unsupported until the real M005 runtime exists. Preserve compact registry/projection payloads and artifact-handle discipline.

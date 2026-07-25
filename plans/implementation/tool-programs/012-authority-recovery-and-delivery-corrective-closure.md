# Tool Programs Milestone 012 — Authority, Recovery, Delivery, and Child-Ownership Corrective Closure

Status: closing

Repository baseline reviewed: `d71a5eee5b31876545981fdb0bd8e437aadee39c` (`main`)

Class: invariant / correctness / authorization / recovery / scheduler ownership / final closure

Primary predecessor:

- `plans/implementation/tool-programs/011-production-correctness-and-ownership-closure.md`

Historical closure record being corrected:

- `plans/closure/tool-programs/011-status.md`

Applicable architecture and policy:

- `plans/adrs/ADR-0001-programmatic-tool-execution-authority.md`
- `architecture/tool_broker.md`
- `architecture/tool_programs.md`
- `architecture/jobs.md`
- `architecture/run_store.md`

Expected closure record:

- `plans/closure/tool-programs/012-status.md`

## 1. Objective

Close the remaining production correctness defects discovered after M011 without redesigning the Tool Programs subsystem or expanding its programmable authority.

M012 must make the following statements true in production code, not only in unit fixtures:

1. a Tool Program carries a real, scope-verifiable authorization decision derived from the existing permission and workspace-policy path;
2. a failed, denied, cancelled, or timed-out nested tool call cannot be persisted or returned as a successful program call;
3. notification claim, injection, acknowledgement, and recovery are coordinated by durable compare-and-set state rather than process-local caches;
4. the scheduler owns descendant cancellation even when an executor future is dropped by timeout, restart, or daemon loss;
5. restart recovery restores or deterministically reconstructs interpreter state from durable, context-bound records and never repeats a durably completed call;
6. child jobs retain durable parent call identity, can be reattached after restart, and return real artifact handles;
7. foreground, background, inspection, and notification consumers read one integrity-checked typed result record with real artifact references;
8. hosted execution is either genuinely reachable through normal runtime construction or explicitly classified as non-production and removed from selectable model-facing policy;
9. process-level and concurrent-service tests prove these mechanisms through public production boundaries.

M012 is the sole strict-closure authority for the findings in this document. M011 remains a useful historical implementation record but is conditionally closed until M012 is accepted.

## 2. Handoff profile for a smaller implementation model

This plan is deliberately prescriptive. Implement it in the listed work-package order.

Rules for the implementing model:

- Do not redesign the parser, IR, provider registry, scheduler, session system, RunStore, or permission UX.
- Do not add mutation-capable tools to the programmable palette.
- Do not replace typed IDs with strings in new code.
- Do not use process-local memory as the source of truth for any state named durable in this plan.
- Do not silently ignore a storage error at a state transition.
- Do not mark M012 closed. Implementation should move the plan to `closing`; an independent closure review creates `plans/closure/tool-programs/012-status.md`.
- Complete one work package, run its focused tests, and commit before starting the next package.
- Keep compatibility decoding additive and fail closed for incomplete historical records.
- Prefer small helper types and repository methods over large inline branches.
- Every new async test must declare an explicit Tokio runtime flavor.
- Use the repository's intended low-concurrency test settings; do not increase test parallelism to mask deadlocks.

Recommended commit sequence:

1. `test(tool-programs): add failing M012 contract coverage`
2. `fix(tool-programs): persist scoped authority grants`
3. `fix(tool-broker): preserve nested failure semantics`
4. `fix(tool-programs): make notification delivery transactional`
5. `fix(scheduler): own Tool Program descendant cancellation`
6. `fix(tool-programs): restore durable replay and child links`
7. `fix(tool-programs): converge typed results and artifacts`
8. `fix(providers): reconcile hosted Tool Program production status`
9. `test(tool-programs): add process-level M012 fault coverage`
10. `docs(plans): move Tool Programs M012 to closing`

## 3. Verified findings and required dispositions

### M012-F01 — Synthetic authority is accepted as verified

Severity: high

Current behavior:

- `src/tool/tool_program_context.rs` manufactures `principal_ref`, `authority_ref`, and `policy_revision` from constants.
- `BrokerAuthority::Verified` is accepted based on enum shape rather than a verifiable permission decision and scope.
- the program adapter passes the authority digest as if it were a verified authority grant.

Required disposition:

- introduce one typed, bounded authority grant/decision representation derived from the existing permission and workspace-policy result;
- persist the grant or a durable reference plus immutable signed/digested decision material with the Tool Program execution context;
- verify tool, caller, effect, workspace/path-policy, principal, manifest, policy revision, and validity at every programmatic Broker call;
- remove constant or locally invented authority identities from production submission.

### M012-F02 — Nested failures can become successful program calls

Severity: high

Current behavior:

- `ToolBroker::execute` converts tool errors into an `Ok(BrokerResult)` containing an error-valued `ToolValue`.
- `BrokerAdapter` treats every `Ok(BrokerResult)` as a successful `CallResult` and permits the interpreter to persist completion and advance.

Required disposition:

- preserve typed terminal status at the Broker boundary;
- direct AgentLoop callers may still receive a displayable error result, but programmatic callers must map denied, cancelled, timed-out, infrastructure-error, schema-error, and tool-error statuses into a typed interpreter failure;
- failed calls must receive an explicit failed/cancelled/timed-out durable call record and must never be inserted into the completed-call replay map.

### M012-F03 — Notification claim and acknowledgement are not transactional

Severity: high

Current behavior:

- claim/ack transitions are decided in process-local maps and then upserted to SQLite;
- two service instances can claim the same pending record;
- persistence failure is logged but the transition reports success;
- injection-before-ack does not have a durable identity that can suppress a duplicate after restart.

Required disposition:

- make SQLite the transition authority;
- use compare-and-set updates with expected state, claim owner, and lease conditions;
- return `Result<TransitionOutcome, NotificationStoreError>` and propagate persistence failure;
- add a durable notification delivery/injection key to the session append path;
- reconcile append-before-ack by detecting the existing durable injected message and acknowledging without reinjecting.

### M012-F04 — Scheduler timeout can strand descendants

Severity: high

Current behavior:

- the scheduler may drop a timed-out Tool Program executor future immediately after cancelling its token;
- child cancellation currently depends on the dropped executor's wait loop observing that token;
- child jobs do not have a canonical durable parent job/attempt/call relationship the scheduler can query.

Required disposition:

- persist generic parent job, attempt, and call lineage for child jobs;
- add scheduler/store operations to list and cancel active descendants;
- invoke descendant cancellation from the scheduler's parent terminalization path, independent of executor-future cleanup;
- prove process-group cleanup and permit convergence.

### M012-F05 — Checkpoints are written but not restored or strongly bound

Severity: high

Current behavior:

- production recovery reloads completed calls but starts a fresh interpreter;
- the durable checkpoint is not loaded into execution;
- call replay checks tool name and raw input only;
- records are not bound to contract version, authority/context digest, manifest digest, workspace revision, selected backend, or control-flow fingerprint.

Required disposition:

- choose and document one recovery mechanism:
  - restore a versioned interpreter snapshot; or
  - restart deterministically from instruction zero while validating a versioned replay cursor and every context/control-flow fingerprint.
- The recommended mechanism for M012 is deterministic restart from instruction zero plus completed-call replay, because it reuses the existing interpreter and avoids serializing arbitrary runtime frames. This mechanism is acceptable only if pure instructions are deterministic and all external calls/child submissions are replay-protected.
- persist and verify the complete replay identity before returning a stored result;
- represent reservation, dispatched ambiguity, completion, failure, cancellation, and replay disposition explicitly.

### M012-F06 — Child-job ownership and recovery are incomplete

Severity: high

Current behavior:

- child keys include sequence but omit durable parent attempt/call fields on the child record;
- a restart cannot query and reattach to the original child through a canonical parent link;
- child results return no artifacts;
- the parent can hold a resource permit required by its child.

Required disposition:

- persist a child-link record before waiting;
- reattach to an existing child on retry/restart;
- propagate real child RunStore/artifact handles into the program call result;
- ensure the Tool Program orchestration job does not consume the same scarce build/test/process permit dimension required by the child;
- prove capacity-one execution cannot deadlock.

### M012-F07 — Typed result integrity and artifact convergence are incomplete

Severity: medium

Current behavior:

- `program_artifacts` is emitted as an empty array;
- result records are file-backed and their stored digest is not recomputed on load;
- terminal state and result persistence are not transactionally correlated with scheduler state.

Required disposition:

- expose bounded call/child/result artifact handles in `ProgramResultRecord` and all public projections;
- verify record version, identity, and digest on every load;
- corrupt or mismatched records must be inspectable as integrity failures and never projected as valid terminal results;
- persist the typed result before job terminal publication, with idempotent recovery if the process dies between those boundaries.

### M012-F08 — Hosted execution is selectable in schema but unreachable in production

Severity: medium

Current behavior:

- the model-facing tool advertises hosted-preferred and hosted-required;
- hosted-required always fails because no transport is attached;
- hosted-preferred always executes native fallback;
- the Responses adapter is not invoked through normal runtime construction.

Required disposition:

Choose exactly one of these paths and record the decision:

- Path A — production integration: inject a real hosted runtime into `ToolProgramExecutor`, route hosted calls through the same Broker, durable call store, cancellation, result, and artifact semantics, and prove it with a production-shaped scripted provider; or
- Path B — truthful non-production classification: remove hosted policies from the model-facing Tool Program parameter schema and normal runtime configuration, retain provider adapter code as experimental/library infrastructure, reject externally supplied hosted policy at production admission, and update architecture/closure documents.

For a smaller implementation model, Path B is the default and recommended M012 scope. Do not attempt Path A unless the existing daemon/provider construction already exposes a narrow transport injection seam and the full required tests can be added without a provider-architecture redesign.

### M012-F09 — Existing evidence is component-level rather than closure-bearing

Severity: medium

Required disposition:

- add process-level daemon restart tests;
- add concurrent SQLite service tests;
- add real scheduler descendant-cancellation and capacity-one tests;
- add a durable session notification injection/ack recovery test;
- ensure closure-bearing tests use public scheduler, daemon, protocol, or session boundaries rather than directly calling only local stores.

## 4. Scope and non-goals

### In scope

- additive core job/session schema changes needed for authority grants, child lineage, Tool Program calls/checkpoints/results, and notification delivery identity;
- Tool Broker authority verification and failure mapping;
- scheduler descendant cancellation and child reattachment;
- durable replay identity and restart behavior;
- typed artifact/result convergence;
- model-facing hosted-policy correction;
- production-path tests, static guards, documentation, registry, and closure evidence.

### Out of scope

- new programmable tools;
- mutation, shell, patch, Git mutation, commit, push, approval-sensitive operations, or subagent spawning from Tool Programs;
- general authorization-policy redesign;
- general scheduler rewrite;
- arbitrary CPython;
- new provider protocols;
- live OpenAI, Eggpool, or ACP service availability as a native correctness dependency;
- performance tuning unrelated to preventing deadlock, leaks, or unbounded state;
- broad cleanup of unrelated clippy or Tokio-test-flavor debt.

## 5. Canonical data contracts

Exact type names may vary, but the following fields and ownership are mandatory.

### 5.1 `ToolAuthorityGrant`

Required fields:

- `schema_version`;
- `grant_id` or decision-event identity;
- `principal_ref`;
- `workspace_id`;
- `workspace_path_policy_id`;
- `session_id` and submitting agent/turn identity where available;
- `permission_mode`;
- `policy_revision`;
- allowed caller class;
- allowed effect class;
- frozen allowed-tool/contract-manifest digest;
- issuance timestamp and optional expiry/revocation generation;
- normalized decision digest.

Rules:

- no secrets or credentials;
- production code must not construct a verified grant from a constant string;
- the Broker must verify grant scope against the current call and frozen manifest;
- missing, unknown-version, expired, revoked, workspace-mismatched, tool-mismatched, or effect-mismatched grants fail closed;
- direct AgentLoop calls and Tool Program calls use the same verification helper.

### 5.2 `ProgramCallRecordV2`

Required fields:

- program ID, scheduler attempt ID, call sequence, and stable call ID;
- call state: `reserved`, `dispatched`, `completed`, `failed_terminal`, `recoverable`, `cancelled`, or `replayed`;
- tool name and contract version;
- effect and idempotency class;
- normalized input hash;
- authority-grant digest;
- execution-context digest;
- manifest digest;
- source and IR digest;
- workspace revision/path-policy identity;
- selected backend;
- reservation, dispatch, completion, and replay timestamps as applicable;
- retry count and ambiguity disposition;
- typed terminal status;
- result digest and artifact handles;
- bounded diagnostic for divergence/failure.

Do not store unbounded raw arguments or raw result bodies in this record. Store bounded summaries and artifact handles.

### 5.3 `ProgramRecoveryCursorV2`

Required fields:

- schema version;
- program ID and scheduler attempt ID;
- IR version/digest and control-flow fingerprint;
- next expected call sequence;
- completed-call cursor;
- budget counters and remaining absolute deadline;
- active child call/link identities;
- latest durable heartbeat;
- recovery disposition and daemon generation.

For deterministic restart-from-zero, the cursor need not serialize arbitrary stack/locals. It must prove that replayed external operations and control-flow call order match the original execution. Any divergence becomes `Recoverable` and stops execution.

### 5.4 `ToolProgramChildLink`

Required fields:

- parent program ID;
- parent job ID and attempt ID;
- parent call ID and sequence;
- child job ID;
- normalized request digest;
- inherited deadline;
- state and terminal status;
- run ID and artifact handles when available;
- created/updated timestamps.

The link must be persisted before the parent begins waiting.

### 5.5 `ProgramNotificationRecordV2`

Add or verify:

- schema version;
- notification ID, program/job/result identity;
- parent session/turn/agent identity;
- state;
- claim owner and lease expiry;
- durable `injection_key`;
- injected session event/message identity where present;
- acknowledged timestamp;
- payload digest and retry count;
- bounded inspection handle.

The SQLite row, not the in-memory cache, decides every transition.

### 5.6 `ProgramResultRecordV2`

Required fields:

- schema version;
- program ID and attempt ID;
- selected backend and fallback/non-production disposition;
- typed terminal status and failure class;
- instruction/call/iteration/byte counters;
- output value or output artifact handle;
- call artifact handles;
- child job/run artifact handles;
- result digest;
- recorded timestamp.

Load must recompute the digest over canonical serialized content and reject mismatch.

## 6. Ordered work packages

## Work package A — Add failing contract tests and freeze current behavior

Primary files:

- `tests/tool_program_m012_authority.rs` — new
- `tests/tool_program_m012_broker_failures.rs` — new
- `tests/tool_program_m012_notifications.rs` — new
- `tests/tool_program_m012_child_ownership.rs` — new
- `tests/tool_program_m012_recovery.rs` — new
- `tests/tool_program_m012_hosted_status.rs` — new
- existing Tool Program test helpers/harness

Steps:

1. Add focused failing tests for every M012 finding before production changes.
2. Use explicit test names matching the closure criteria IDs in section 8.
3. Keep tests deterministic and low-concurrency.
4. Add helper constructors only under `tests/common` or existing harness modules; do not add a second runtime.
5. Record the expected initial failures in the implementation commit message or PR description, not in the final closure record.

Minimum red tests:

- a constant/synthetic authority grant is rejected;
- a permission-denied nested read does not produce a completed call;
- two pool-backed services concurrently claim one notification and exactly one succeeds;
- SQLite write failure is returned, not logged-and-ignored;
- parent scheduler timeout cancels an already-running child after the parent executor future is dropped;
- restart reattaches to an existing child instead of creating another child;
- replay with changed authority or contract version fails recoverably;
- corrupt result digest is rejected;
- production model-facing schema does not advertise unreachable hosted execution under Path B.

Acceptance gate A:

- tests compile;
- each test fails for the intended mechanism rather than setup failure;
- no production behavior is changed in this commit.

## Work package B — Real scoped authority grants

Primary files:

- `src/agent/loop.rs`
- permission-checker result types used by AgentLoop
- `src/tool/backend.rs`
- `src/tool/broker.rs`
- `src/tool/tool_program_context.rs`
- `src/tool/tool_program.rs`
- `src/scheduler/tool_program_executor.rs`
- `crates/codegg-core/src/jobs/mod.rs`
- session/job schema migration files
- `tests/tool_program_m012_authority.rs`

Steps:

1. Define `ToolAuthorityGrant` in the lowest appropriate shared crate without introducing a dependency cycle.
2. Build the grant from the actual successful permission/path-policy decision in AgentLoop. Preserve the permission mode and policy revision that produced the decision.
3. Include the frozen manifest/contract digest in the Tool Program grant scope.
4. Persist the grant or durable grant reference plus full normalized decision material in `ToolProgramExecutionContext`/job payload.
5. Replace `BrokerAuthority::Verified { authority_ref, ... }` with a representation that contains or resolves the complete grant.
6. Add `verify_authority_grant(contract, caller, input/path context, workspace, manifest)` in `ToolBroker`.
7. Verify the grant on every direct and programmatic call. A replayed call is also revalidated against the immutable persisted grant and frozen context; do not silently expand authority from current broader policy.
8. Delete constant `local-agent` authority construction and constant authority hashes from production code.
9. Historical M011 jobs without a valid grant remain inspectable but cannot execute or resume.
10. Keep the read-only programmable palette unchanged.

Concrete examples:

- Grant permits `read`, workspace `ws-1`, path policy `workspace:ws-1`, effect `read_only`. Calling `grep` fails unless `grep` is in the frozen manifest.
- Grant for workspace `ws-1` cannot execute with a job attached to `ws-2`.
- Grant for direct caller cannot be reused for `ToolCaller::Program` unless its caller scope includes programmatic execution.
- A changed policy revision does not mutate the historical grant. Revocation semantics, if available, may deny it explicitly; otherwise immutable submission authority is used for restart.

Acceptance gate B:

- no production path constructs verified authority from a constant or arbitrary digest;
- direct and programmatic calls use one verification helper;
- scope mismatch fails before tool invocation;
- missing/unknown-version grants fail closed;
- targeted authority tests pass;
- static search finds no obsolete boolean or synthetic-authority bypass.

## Work package C — Preserve Broker and interpreter failure semantics

Primary files:

- `src/tool/broker.rs`
- `src/scheduler/tool_program_executor.rs`
- `crates/codegg-core/src/tool_program/interpreter.rs`
- `src/tool/tool_program_ledger.rs` or its replacement repository
- `tests/tool_program_m012_broker_failures.rs`
- existing Broker integration tests

Steps:

1. Define an explicit mapping from `ToolTerminalStatus` to program call outcome.
2. Keep direct AgentLoop presentation compatibility, but expose a helper such as `BrokerResult::into_programmatic_outcome()` that returns failure for all non-success statuses.
3. Map cancellation distinctly from timeout, denial, schema failure, and infrastructure/tool failure.
4. Add a durable failed call record before returning the interpreter error.
5. Only `ToolTerminalStatus::Success` may become `CompletedCall` and enter the replay-completed map.
6. Output-schema failure must be a failed call, not an infrastructure-success wrapper.
7. Preserve artifact handles attached to failure records when safe and bounded.
8. Ensure retry logic obeys the tool contract's idempotency/retry class. Do not retry policy denial, schema mismatch, or cancellation.
9. Ensure a failed nested call produces a failed outer structured Tool Program result unless restricted-Python source explicitly catches a supported typed error; do not invent catch semantics if the language does not support them.

Required mapping:

| Broker/tool outcome | Program call state | Interpreter outcome |
|---|---|---|
| success | completed | return typed result |
| permission denied | failed_terminal | policy/permission failure |
| input/output schema mismatch | failed_terminal | contract failure |
| cancelled | cancelled | `InterpreterError::Cancelled` |
| timed out | recoverable or failed terminal per contract | timeout failure |
| infrastructure/tool error | recoverable only when contract explicitly permits retry; otherwise failed_terminal | typed backend/execution failure |

Acceptance gate C:

- denied/timed-out/cancelled/error Broker outcomes never increment `calls_completed`;
- they are never replayed as successful completed calls;
- direct AgentLoop error display remains usable;
- output-schema mismatch is typed and persisted;
- targeted Broker failure tests pass.

## Work package D — Transactional notification claim, injection, and acknowledgement

Primary files:

- `crates/codegg-core/src/session/schema.rs`
- `src/scheduler/tool_program_notifications.rs`
- session event/message persistence path used by AgentLoop
- `src/agent/loop.rs`
- `src/agent/turn_runtime.rs`
- `src/core/daemon.rs`
- `tests/tool_program_m012_notifications.rs`
- `tests/storage_migrations.rs`

Steps:

1. Add an additive migration for notification schema version and delivery identity fields.
2. Refactor notification storage behind a repository interface whose production implementation is SQLite-backed.
3. `claim` must execute one SQL compare-and-set operation similar to:

   ```sql
   UPDATE tool_program_notification
      SET state = 'claimed', claim_owner = ?, claim_lease_until = ?, updated_at = ?
    WHERE notification_id = ?
      AND state = 'pending';
   ```

   Success requires exactly one affected row.
4. Permit reclaim only through an explicit expired-lease transition performed transactionally.
5. `acknowledge` must require `state = 'claimed'` and the expected claim owner or delivery identity.
6. Return storage errors. Do not log and report success.
7. Create a deterministic `injection_key` from notification ID plus parent session identity.
8. Append the bounded notification message/control event through the durable session path with that identity.
9. After append succeeds, acknowledge the notification.
10. On restart after append but before acknowledgement, query the durable session/event path for `injection_key`; if present, acknowledge without appending again.
11. Update caches only after database commit. Caches may accelerate reads but cannot decide transitions.
12. Use one daemon-scoped production notification repository where practical; correctness must still hold with two service instances sharing the same pool.
13. Keep empty-session rejection and per-session bounds.

Required race tests:

- two service instances claim one pending notification concurrently: one true, one false;
- claimant dies after claim: lease expiry makes it claimable once;
- database unavailable during claim: both callers receive an error and state remains pending;
- append succeeds then process dies before ack: restart acknowledges existing injection and does not append again;
- ack succeeds then process restarts: no delivery is recreated;
- duplicate terminal result insertion preserves one notification and validates payload digest.

Acceptance gate D:

- all transition decisions are durable CAS operations;
- no transition reports success after persistence failure;
- exactly one durable session injection exists across every crash boundary;
- delivered acknowledgement survives restart;
- concurrent SQLite tests pass repeatedly.

## Work package E — Scheduler-owned descendant lineage, cancellation, and permits

Primary files:

- `crates/codegg-core/src/jobs/mod.rs`
- core job-store trait and SQLite/in-memory implementations
- job schema migration files
- `src/scheduler/scheduler.rs`
- `src/scheduler/submission.rs`
- `src/scheduler/tool_program_executor.rs`
- managed process/test/build executors as needed for artifact retrieval and process cleanup
- `tests/tool_program_m012_child_ownership.rs`
- scheduler contention/cancellation tests

Steps:

1. Add optional typed `parent_job_id`, `parent_attempt_id`, and `parent_call_id` to generic `NewJob`/`JobRecord`, or add an equivalent canonical child-link table. Do not hide this relationship only in labels.
2. Persist the relationship atomically with child creation or immediately before enqueue.
3. Add store queries for active descendants of a job/attempt.
4. Add `request_cancel_descendants(parent_job_id, reason)` to scheduler ownership.
5. Call descendant cancellation from every parent terminal path where descendants must not continue: explicit cancel, scheduler timeout, lost-worker reconciliation, unrecoverable parent failure, and daemon generation abandonment.
6. Do not rely on the Tool Program executor future remaining alive to send cancellation.
7. Persist `ToolProgramChildLink` before waiting and update it with child terminal/run/artifact state.
8. On replay/restart, look up the link by parent program/attempt/call identity. Reattach to the existing child; never submit a replacement solely because process memory was lost.
9. Ensure the Tool Program orchestration job does not hold the scarce permit dimension required by build/test/managed child work. Prefer a lightweight orchestration permit plus explicit child resource requests.
10. Propagate the narrower parent deadline and parent cancellation.
11. Retrieve child RunStore/artifact handles and include them in the typed call result.
12. Verify managed process groups are terminated before parent cancellation is considered converged.

Required examples:

- sequence 3 and sequence 4 submit identical `cargo test` configs: two distinct children;
- replay of sequence 3 reuses its original child;
- scheduler capacity for build/test is one: parent program can submit and await the child without deadlock;
- outer scheduler timeout drops the Tool Program executor future: scheduler still cancels the active child and process group;
- daemon restart while child is running: recovered parent reattaches and receives the existing result.

Acceptance gate E:

- every child has queryable parent program/job/attempt/call lineage;
- parent cancellation/timeout/lost-worker terminalization cancels all active descendants independently of executor cleanup;
- no duplicate child is created during replay;
- capacity-one test completes without deadlock;
- process, job, attempt, and permit counts return to baseline;
- child artifacts resolve through their handles.

## Work package F — Durable replay identity and recovery cursor

Primary files:

- `crates/codegg-core/src/tool_program/interpreter.rs`
- `src/tool/tool_program_ledger.rs`
- `src/scheduler/tool_program_executor.rs`
- Tool Program storage/migration modules
- `tests/tool_program_m012_recovery.rs`
- existing fault-injection tests

Implementation direction:

Move authoritative call/recovery metadata to SQLite or another already-canonical transactional store. File-backed artifact bodies may remain file-backed. Do not retain whole-file JSON read/modify/rename as the sole authority for concurrent or crash-sensitive call state.

Steps:

1. Add additive tables or versioned repository records for call state and recovery cursor.
2. Convert ledger APIs to async repository operations where needed.
3. Reserve a call transactionally before dispatch.
4. Mark `dispatched` immediately before entering the external Broker/child boundary.
5. Persist completed/failed/cancelled outcome and artifact references before interpreter advancement.
6. Persist the recovery cursor after every terminal external-call record and explicit checkpoint instruction.
7. On attempt recovery, load:
   - program/source/IR/manifest/context/grant fingerprints;
   - completed external calls;
   - unresolved dispatched reservations;
   - recovery cursor;
   - active child links;
   - remaining absolute deadline.
8. Restart the interpreter deterministically from instruction zero, preload completed calls, and verify each call sequence against all stored fingerprints before replay.
9. For a `dispatched` read-only safe-repeat call without completion, repeat only if the frozen contract explicitly says safe repeat. Record the ambiguity and replay disposition.
10. Never repeat a child submission with a durable child link; reattach instead.
11. On any tool/contract/input/authority/context/manifest/source/IR/workspace/control-flow mismatch, persist `Recoverable` with a bounded divergence diagnostic and stop.
12. Recompute remaining deadline from the original absolute deadline. Restart does not reset timeout.
13. Reconcile abandoned `Running` attempts by daemon generation before resuming.
14. Remove or demote old file journal authority only after migration and compatibility tests pass.

Crash-window tests:

- before reservation;
- after reservation before dispatch;
- after dispatched marker before Broker return;
- after Broker return before completion persistence;
- after completion persistence before recovery cursor;
- after recovery cursor before scheduler terminal result;
- while awaiting a child;
- after typed result persistence before job terminal publication.

Acceptance gate F:

- no durably completed call is executed twice after process restart;
- an unresolved safe-repeat read follows explicit ambiguity policy and is auditable;
- child submissions reattach rather than repeat;
- changed grant, contract, context, manifest, source, IR, workspace, or call order fails recoverably;
- original absolute deadline remains authoritative;
- no attempt remains indefinitely `Running` after daemon-generation reconciliation;
- process-level crash-window tests pass.

## Work package G — Typed result and artifact convergence

Primary files:

- `src/tool/tool_program_result.rs`
- Tool Program result storage/migration modules
- `src/scheduler/tool_program_executor.rs`
- `src/tool/tool_program.rs`
- `src/scheduler/tool_program_notifications.rs`
- Tool Program projection/inspection handlers and DTOs
- `src/context/artifact.rs`
- `tests/tool_program_context_artifacts.rs`
- `tests/tool_program_m012_recovery.rs`

Steps:

1. Extend the typed result record with output, call, and child artifact handles.
2. Populate call artifacts from Broker results and child artifacts from child RunStore/completion data.
3. Remove unconditional `program_artifacts: []` projection.
4. Keep raw bodies outside the parent transcript; expose bounded previews and handles only.
5. Canonically serialize result content and compute digest excluding the digest field itself.
6. Recompute and compare digest on load.
7. Reject unknown versions, identity mismatch, digest mismatch, oversized data, symlinks, and invalid handles.
8. Make foreground wait, background notification, list/inspect/call-page, and recovery consume the same verified record.
9. Persist result before scheduler terminal publication. If terminal publication is replayed after restart, reuse the same result record and do not create a conflicting result.
10. Keep human summary presentation-only.

Acceptance gate G:

- foreground/background/inspection expose the same result identity, counters, terminal status, selected backend, and artifact handles;
- every exposed artifact handle resolves and verifies its content digest;
- corrupt result records fail closed and remain diagnostically inspectable;
- intermediate raw call outputs remain absent from the parent transcript;
- result persistence and terminal publication recover idempotently.

## Work package H — Hosted runtime truthfulness decision

Primary files:

- `src/tool/tool_program.rs`
- `src/tool/backend.rs`
- `src/tool/factory.rs`
- `src/scheduler/tool_program_executor.rs`
- `src/core/daemon.rs`
- `src/agent/turn_runtime.rs`
- `crates/codegg-providers/src/responses_api.rs`
- provider and Tool Program architecture docs
- `tests/tool_program_m012_hosted_status.rs`

Default implementation: Path B, explicit non-production classification.

Path B steps:

1. Remove `native_preferred`, `hosted_preferred`, and `hosted_required` from the model-facing `tool_program` parameter schema unless a real hosted runtime is attached.
2. Production `ToolProgramExecutionContext` must persist `native_only`.
3. Reject externally supplied non-native policy at admission with a clear unsupported/non-production error.
4. Keep `HostedProgramAdapter` and policy types in `codegg-providers` as experimental/library code.
5. Update docs to state that normal daemon Tool Programs are native-only and that M009 adapter integration remains future interoperability work.
6. Do not record `native_fallback` when no hosted attempt occurred; record `native`.
7. Keep provider tests for the adapter but do not cite them as production runtime evidence.

Path A may replace Path B only when all of the following are implemented:

- daemon/runtime construction injects a hosted transport into `ToolProgramExecutor`;
- normal provider capability resolution selects it;
- hosted nested calls use the same scoped grant, Broker, call store, recovery, notification, result, and artifact services;
- continuation/fingerprint state is durable;
- cancellation/restart tests pass through the production runtime.

Acceptance gate H:

- no model-facing or config-selectable policy points to an unreachable backend;
- result/provenance reflects the backend actually executed;
- documentation, parameter schema, runtime behavior, and tests agree;
- under Path B, production hosted-required/preferred is explicitly unsupported rather than silently falling back.

## Work package I — Process-level and concurrency closure harness

Primary files:

- existing Tool Program harness skill and tests
- public daemon/core-stdio or daemon-socket integration harness
- `tests/tool_program_m012_process_recovery.rs` — new
- `tests/tool_program_m012_notifications.rs`
- `tests/tool_program_m012_child_ownership.rs`
- static guard scripts

Steps:

1. Add a process-level fixture that launches the real daemon with a temporary workspace and SQLite database.
2. Submit Tool Programs through a public protocol boundary, not by calling executor methods directly.
3. Add deterministic failpoints around call reservation, dispatch, completion, recovery cursor, result persistence, session notification injection, and job terminal publication.
4. Kill the daemon process at each failpoint and restart against the same workspace/database.
5. Add a scripted read-only tool that counts physical executions in durable test state so duplicate execution is observable.
6. Add a scripted managed child process that exposes its PID/process group and writes a terminal marker.
7. Run concurrent notification claimants using separate service instances and pool connections.
8. Add capacity-one scheduler/resource configuration.
9. Repeat the mixed restart/cancel/claim scenario enough times to detect races; use a deterministic seed and record it on failure.
10. Strengthen static guards to reject:
    - constant/synthetic verified authority construction;
    - programmatic Broker success mapping that ignores terminal status;
    - notification transition success without durable rows-affected confirmation;
    - child jobs lacking parent call identity;
    - Tool Program production policies advertising unreachable hosted execution;
    - end-of-run-only call persistence;
    - summary-string semantic parsing;
    - process-local-only durable state.

Acceptance gate I:

- all closure-bearing restart scenarios pass through the production daemon/protocol path;
- no duplicate completed read call is observed;
- notification has one durable injection;
- descendants and processes terminate on parent timeout/cancel;
- capacity-one execution converges;
- storage, tasks, processes, permits, claims, and artifact writers return to baseline;
- static guards pass.

## Work package J — Documentation, registry, and closure handoff

Primary files:

- `architecture/tool_broker.md`
- `architecture/tool_programs.md`
- `architecture/jobs.md`
- `architecture/run_store.md`
- `architecture/provider.md`
- `plans/subsystems/tool-programs-correctness-closure-addendum.md`
- `plans/subsystems/tool-programs-roadmap.md`
- `plans/registry.md`
- `plans/implementation/tool-programs/012-authority-recovery-and-delivery-corrective-closure.md`

Steps:

1. Document the real authority-grant lifecycle and verification scope.
2. Document Broker failure mapping for direct versus programmatic consumers.
3. Document transactional notification CAS and injection-before-ack recovery.
4. Document parent/child lineage, scheduler-owned descendant cancellation, and resource ownership.
5. Document deterministic restart-from-zero and the complete replay fingerprint.
6. Document result integrity and artifact ownership.
7. Document the selected hosted disposition truthfully.
8. Update M012 status to `closing` only after production code and required tests land.
9. Update the registry to move M012 from ready to closing and identify any remaining blocker.
10. Do not create or mark `plans/closure/tool-programs/012-status.md` closed as part of implementation. An independent review must compare the code and evidence to section 8.

Acceptance gate J:

- architecture docs match production behavior;
- M011 is described as historical conditional closure;
- M012 is the only active strict-closure authority;
- registry, roadmap/addendum, plan status, and implementation state agree;
- no closure statement relies on unrun live-provider evidence.

## 7. Required test commands

The implementing model must run focused commands after each work package and record exact results for handoff. Adapt package names only when the repository requires it.

Required focused commands:

```text
cargo fmt --all -- --check
cargo check -p codegg --all-targets
cargo check --workspace --all-targets --all-features
cargo test --test storage_migrations -- --test-threads=1
cargo test --test tool_program_m012_authority -- --test-threads=1
cargo test --test tool_program_m012_broker_failures -- --test-threads=1
cargo test --test tool_program_m012_notifications -- --test-threads=1
cargo test --test tool_program_m012_child_ownership -- --test-threads=1
cargo test --test tool_program_m012_recovery -- --test-threads=1
cargo test --test tool_program_m012_hosted_status -- --test-threads=1
cargo test --test tool_program_m012_process_recovery -- --test-threads=1
cargo test --test tool_broker_integration -- --test-threads=1
cargo test --test tool_program_fault_injection -- --test-threads=1
cargo test --test tool_program_notifications -- --test-threads=1
cargo test --test tool_program_runtime -- --test-threads=1
cargo test --test tool_program_context_artifacts -- --test-threads=1
cargo test -p codegg --lib tool::broker -- --test-threads=1
```

Required repository guards include all existing ownership/security guards plus newly added M012 guards.

Broader workspace tests must use the repository's intended constrained concurrency. A pre-existing unrelated failure may be documented, but every M012-owned test, warning, migration, and static guard must be clean.

## 8. Explicit closure criteria

M012 may be marked closed only when every criterion below is supported by production code and named evidence. A prose assertion without a mechanism-faithful test is not evidence.

### Authority and Broker

- **C-01:** No production code creates a verified authority grant from a constant, arbitrary digest, or caller-supplied assertion.
- **C-02:** Every Tool Program submission persists a versioned authority grant derived from the real permission/path-policy decision.
- **C-03:** Every nested Broker call verifies principal, workspace/path policy, caller class, effect class, tool/contract manifest, and policy revision scope.
- **C-04:** Missing, stale/invalid, unknown-version, mismatched, or revoked grants fail closed before tool invocation.
- **C-05:** Denied, failed, cancelled, timed-out, and schema-invalid nested calls cannot become successful `CompletedCall` records.
- **C-06:** Only successful Broker terminal status increments completed-call counters or enters replay-completed state.

### Notification delivery

- **C-07:** SQLite compare-and-set is the authority for claim, reclaim, acknowledgement, suppression, and failure transitions.
- **C-08:** Two concurrent service instances cannot both claim the same pending notification.
- **C-09:** A database transition error is returned to the caller and never reported as success.
- **C-10:** Restart before claim, after claim, after durable injection, and after acknowledgement yields exactly one durable parent-session message/control event.
- **C-11:** Delivered or suppressed notifications are never recreated by terminal-job recovery.

### Scheduler and child ownership

- **C-12:** Every child is durably correlated to parent program, job, attempt, call ID, and sequence before the parent waits.
- **C-13:** Parent cancellation, scheduler timeout, lost-worker reconciliation, and daemon-generation abandonment cancel active descendants without relying on the parent executor future.
- **C-14:** Replay/restart reattaches to the existing child and does not create a duplicate.
- **C-15:** Two deliberate identical child instructions at different sequences create two children.
- **C-16:** Child deadline never exceeds the parent deadline.
- **C-17:** Capacity-one build/test/process resources do not deadlock a waiting Tool Program.
- **C-18:** Descendant process groups, jobs, attempts, and permits converge to baseline after cancel/timeout.

### Replay, results, and artifacts

- **C-19:** Call reservation, dispatch state, terminal outcome, and recovery cursor are durable before dependent interpreter state advances.
- **C-20:** Restart never physically re-executes a durably completed call.
- **C-21:** Replay validates tool/contract, input, authority, context, manifest, source/IR, workspace/path policy, backend, and call-order fingerprints.
- **C-22:** Replay divergence persists an inspectable recoverable result and stops execution.
- **C-23:** Original absolute deadline remains authoritative across restart.
- **C-24:** Foreground, background, notification, and inspection read the same integrity-checked typed result.
- **C-25:** Result digest is recomputed on load; corruption or identity mismatch fails closed.
- **C-26:** Real call and child artifact handles are present, bounded, resolvable, and digest-verifiable; `program_artifacts` is not an unconditional empty placeholder.

### Hosted truthfulness and evidence

- **C-27:** Production configuration and model-facing schema expose only backends reachable through normal runtime construction.
- **C-28:** Under recommended Path B, production is explicitly native-only and no silent `native_fallback` is recorded for an unattempted hosted path.
- **C-29:** All closure-bearing restart, notification, descendant, and capacity tests exercise public production boundaries.
- **C-30:** All M012-focused tests, migrations, formatting, compilation, and static guards pass.
- **C-31:** No unresolved high or medium correctness, authorization, recovery, notification, child-ownership, resource, result-integrity, or evidence finding remains.
- **C-32:** `plans/closure/tool-programs/012-status.md`, roadmap/addendum, architecture docs, and registry agree, and M011 remains labeled historical conditionally closed.

## 9. Required closure evidence matrix

The eventual M012 closure record must contain a table with one row for every C-01 through C-32 criterion and include:

- implementation file/type/function;
- test name and command;
- commit SHA containing the mechanism;
- pass/fail result;
- any operational limitation;
- reviewer disposition.

It must additionally record:

- migration version and forward/backward compatibility behavior;
- exact process-level failpoints exercised;
- number of repeated race/fault iterations and deterministic seeds;
- resource baseline and post-test counts;
- hosted Path A or Path B decision;
- unrelated repository-wide failures, with evidence that they do not mask M012 failures;
- confirmation that no credentials, private endpoints, raw hidden reasoning, or unbounded sensitive arguments/results were committed.

## 10. Final handoff definition

Implementation handoff is complete when:

1. work packages A through I are implemented and committed in reviewable units;
2. all required focused tests and guards pass;
3. architecture documentation is updated;
4. this plan is moved from `ready`/`active` to `closing`;
5. the registry identifies M012 as closing and contains no contradictory strict M011 claim;
6. a reviewer can create `plans/closure/tool-programs/012-status.md` without inferring mechanisms or rerunning undocumented commands.

Strict subsystem closure is not complete until an independent review verifies C-01 through C-32 and accepts the M012 closure record.
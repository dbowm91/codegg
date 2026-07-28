# Tool Programs Milestone 014 — Production-Boundary and Process-Evidence Closure

Status: closing

Class: corrective implementation / authorization / durable recovery / recursive ownership / artifact integrity / process evidence / governance closure

Baseline reviewed:

- `58e87ff3d82508037ae4912df2ae9b9b8a4ef090`

Predecessor records:

- `plans/implementation/tool-programs/013-production-authority-descendant-and-recovery-closure.md`
- `plans/closure/tool-programs/013-status.md`
- `plans/subsystems/tool-programs-correctness-closure-addendum.md`

Target closure record:

- `plans/closure/tool-programs/014-status.md`

## 1. Objective

Correct the remaining production-path defects after M013 and establish mechanism-faithful closure through the real daemon, scheduler, SQLite, Broker, interpreter, session, RunStore, and artifact boundaries.

M014 is complete only when all of the following are true in the normal production path:

1. Tool Program authority is derived from the actual accepted permission and workspace path-policy decision, not synthesized from identity strings or hashes;
2. the exact frozen contract catalog used at admission is persisted and verified consistently by the Broker;
3. ordinary nested calls succeed when authorized and fail closed on any real grant, manifest, contract, policy, or path mismatch;
4. the latest valid interpreter checkpoint is loaded and restored before execution resumes;
5. restored state includes every value needed to continue safely, including locals/control state, budgets, call sequence, pending child identity, replay fingerprint, and original absolute deadline;
6. child lineage contains durable parent program, job, attempt, canonical call ID, instruction sequence, and relation kind, survives every job state transition, and upgrades existing databases through a new migration;
7. the scheduler enumerates and cancels descendants recursively, reconciles process groups and permits, and reattaches existing children after restart;
8. notification persistence and recovery return errors rather than logging-and-continuing, use correctly labeled SHA-256 identities, and provide durable idempotent parent-session injection;
9. call, child, and output artifacts are stored through canonical artifact/RunStore abstractions and are resolvable and digest-verifiable;
10. replay and checkpoint storage are safe across daemon restart and overlapping process lifetimes, not merely protected by a process-local mutex;
11. a real daemon process is submitted to through a public protocol, killed at deterministic failpoints, restarted against the same durable state, and observed to converge correctly;
12. implementation and closure review remain separate, with accurate commit and test evidence.

## 2. Handoff profile for a smaller implementation model

This plan is intentionally prescriptive. Follow it in order.

### 2.1 Required execution style

- Add failing production-path tests before each behavior change.
- Keep work packages in separate commits according to the required commit sequence.
- Inspect the current permission, path-policy, contract-catalog, artifact, RunStore, session-insertion, daemon-protocol, and managed-process abstractions before adding new types.
- Reuse the existing production authority and storage boundaries. Do not add a second authorization engine, scheduler, session database, artifact namespace, or daemon protocol.
- Use typed values for security-relevant caller, effect, relation, and decision fields where existing enums are available.
- Propagate storage and integrity failures. Logging a warning and returning success is forbidden for closure-bearing operations.
- Run repository tests with the repository's intentional low-concurrency constraints.
- Move this plan only to `closing` after implementation and required tests pass.
- Do not create, approve, or mark `plans/closure/tool-programs/014-status.md` closed. Closure review is a separate task.

### 2.2 Non-goals

Do not:

- redesign the restricted Python language, parser, compiler, or IR beyond the minimum checkpoint state required for correct resume;
- broaden the Tool Program palette beyond the existing read-only and safe scheduler-owned child operations;
- add shell, patch, Git mutation, commit, push, destructive, approval-sensitive, or subagent tools;
- implement hosted Tool Programs;
- replace the scheduler, JobStore, session store, RunStore, or artifact store wholesale;
- retain a compatibility path that silently fabricates authority when the real decision is unavailable;
- call an in-process object reconstruction a daemon restart test;
- treat field existence, comments, static search, or manually constructed fixtures as proof of production behavior;
- weaken a binary closure criterion because a test seam or failpoint does not yet exist.

### 2.3 Default architecture decisions

Use these decisions unless an existing production abstraction provides a smaller equivalent:

- The direct `tool_program` invocation is authorized once through the normal AgentLoop/Broker permission and path-policy boundary. That accepted decision is frozen into a versioned grant before scheduler submission.
- The grant carries a durable decision reference and a complete bounded verification snapshot. Identity-derived hashes are correlation values only, never authorization evidence.
- The contract catalog snapshot is canonicalized once at submission. The same canonical digest function is used at submission, executor admission, and every nested Broker call.
- SQLite is authoritative for jobs, lineage, notifications, delivery identity, and daemon recovery. Tool Program replay/checkpoint state should also move to SQLite unless a repository-standard cross-process locking abstraction already provides equivalent guarantees.
- A new schema change must use the next migration version. Do not modify an already-applied migration and assume existing databases will rerun it.
- The scheduler owns recursive descendant reconciliation. Parent executor cleanup is a secondary defense, not the source of truth.
- Large output and child/call artifacts use the canonical artifact store or RunStore and return real resolvable handles plus SHA-256 digests.
- Production remains `native_only`.

## 3. Verified baseline findings

The review of `58e87ff3d82508037ae4912df2ae9b9b8a4ef090` found the following unresolved defects.

### F01 — Authority remains synthesized rather than decision-derived

`to_core_context()` still constructs principal, authority, path-policy, and policy-revision values from program/workspace/session/agent strings. `build_authority_grant()` hashes that synthetic context. Submission timing improved, but the grant is still not the persisted result of the actual accepted permission and path-policy decision.

### F02 — Contract snapshot construction is inconsistent in production

The normal submission path hashes an empty contract summary into the grant. Broker verification hashes the concrete invoked `ToolContract` and requires equality. Production-authorized nested calls can therefore fail even though manually constructed authority fixtures pass.

### F03 — Checkpoint restore exists as an API but is not used

The executor loads completed calls only. It does not load the latest durable checkpoint or call `restore_checkpoint()`.

### F04 — Checkpoint state is insufficient for direct resume

The checkpoint stores a locals hash rather than the bounded locals/control state. The restore helper sets the program counter and budgets without reconstructing locals, loop/control frames, or a pending child wait. Jumping to the saved program counter can therefore resume with invalid interpreter state.

### F05 — Deadline recovery is not bound in the production fingerprint

The production executor constructs `ReplayFingerprint` with `original_deadline_millis: None`. A restarted program is not proven to retain the original absolute deadline.

### F06 — Lineage is incomplete and erased by state transitions

The durable domain exposes only parent job, attempt, and call fields. It lacks parent program ID, parent instruction sequence, and relation kind. Child submission still derives `parent_call_id` from operation type. Several in-memory enqueue, attempt, cancellation, blocking, and recovery updates explicitly replace lineage fields with `None`.

### F07 — Existing database upgrades do not receive the lineage schema

Lineage `ALTER TABLE` statements were added to an old migration. Databases already beyond that version do not rerun it. A new migration version is required.

### F08 — Descendant enumeration is direct-only

`find_descendants()` and `cancel_descendants()` process direct children only. M014 requires recursive descendants, restart reconciliation, and bounded convergence of jobs, attempts, process groups, permits, and workspace leases.

### F09 — Child and output artifacts are not canonical

Child artifact records still contain no run ID, artifact handle, or digest. Large output is written directly to a constructed filesystem path, a `ctx://` string is fabricated, and write failure only logs a warning. This bypasses the canonical artifact/RunStore boundary and does not fail closed.

### F10 — Notification persistence still swallows errors and recovery uses MD5

`persist_record()` returns `()` and logs serialization/SQLite failures. Recovery computes MD5 payload digests despite the field being documented as SHA-256. Durable injection identity is still primarily embedded in JSON rather than enforced through schema-level uniqueness and idempotent session insertion.

### F11 — Replay journal locking is process-local

A `DashMap` mutex prevents same-process writers from racing, but does not protect overlapping daemon processes or crash/restart boundaries. The journal remains a whole-file read/modify/write store.

### F12 — Process-level evidence was explicitly deferred

The M013 process suite reconstructs stores in one test process. It does not start the real daemon, submit through a public protocol, activate failpoints, kill the daemon, restart against the same database, or observe managed child process cleanup.

### F13 — Governance closure was self-accepted and internally contradictory

The implementation pass created and accepted its own M013 closure record despite the plan prohibiting that. The record marks strict closure while listing a mandatory process criterion as deferred and making mechanism claims that do not match production code.

## 4. Required commit sequence

Use this sequence. Do not squash it before independent review.

1. `test(tool-programs): add failing M014 production authority pipeline coverage`
2. `fix(tool-programs): derive grants from accepted permission decisions`
3. `fix(tool-programs): freeze and verify canonical contract snapshots`
4. `test(tool-programs): add failing checkpoint resume and deadline coverage`
5. `fix(tool-programs): persist and restore complete interpreter checkpoints`
6. `fix(tool-programs): make replay state cross-process safe`
7. `test(jobs): add M014 lineage migration and transition preservation coverage`
8. `fix(jobs): add complete durable lineage and upgrade migration`
9. `test(scheduler): add recursive descendant reattachment and convergence coverage`
10. `fix(scheduler): own recursive descendant reconciliation`
11. `test(tool-programs): add notification persistence and injection fault coverage`
12. `fix(tool-programs): make notification delivery fail-closed and idempotent`
13. `test(tool-programs): add canonical call child and output artifact coverage`
14. `fix(tool-programs): route all result artifacts through canonical stores`
15. `test(tool-programs): add real daemon kill restart failpoint harness`
16. `docs(plans): move Tool Programs M014 to closing`

If a compile-only schema propagation change must accompany its migration commit, keep it in the same commit and explain why.

## 5. Work package A — Real production authority decision

### A1. Locate the actual decision boundary

Inspect at minimum:

- `src/agent/loop.rs`
- `src/tool/broker.rs`
- `src/tool/tool_program.rs`
- `src/tool/tool_program_context.rs`
- `src/tool/backend.rs`
- permission and workspace path-policy types used by direct tool calls
- `plans/adrs/ADR-0001-programmatic-tool-execution-authority.md`

Identify the exact accepted decision output used for the direct `tool_program` call. This may be a typed decision object, decision ID plus revision, or a bounded immutable snapshot. Do not infer acceptance from `permission_mode`, session identity, a non-empty path-policy string, or a locally computed hash.

### A2. Carry the decision into structured execution context

Extend the existing `ToolExecutionContext` or an adjacent typed production context with the minimum non-secret fields required to prove:

- decision ID/reference;
- decision outcome (`allowed` only for submission);
- principal identity;
- workspace ID;
- canonical workspace root/path-policy ID and revision;
- permission policy revision;
- caller class;
- maximum effect class;
- session/turn/agent identity where applicable;
- issued-at, expiry, and revocation reference/state.

The AgentLoop/Broker permission path populates this only after the direct invocation is accepted. Direct construction by model input is forbidden.

### A3. Build and persist the immutable grant before submission

Replace synthetic grant creation with a constructor that requires the accepted decision snapshot. Fail submission if it is absent, denied, malformed, stale, expired, or cannot be tied to the current workspace/path policy.

Persist either:

- the complete bounded immutable grant in `JobPayload::ToolProgram`; or
- a canonical durable grant record plus immutable reference and digest.

The executor may deserialize and verify the grant. It may not synthesize, repair, widen, or replace it.

### A4. Distinguish correlation from authorization

Values such as program ID, invocation key, source digest, context digest, and correlation ID may bind or correlate the decision, but they cannot establish that permission was granted.

Delete or rename helpers whose names imply authority while they only hash context. Preserve backward readers only if they fail closed for execution.

### A5. Production-path tests

Create `tests/tool_program_m014_authority_pipeline.rs` using the normal AgentLoop/Broker/tool submission path. Prove:

- an accepted real decision creates the persisted grant;
- a denied decision creates no job;
- missing decision context creates no job;
- identity strings and hashes alone cannot create a valid grant;
- tampering every security-relevant field fails before tool execution;
- stale path-policy or permission revision fails;
- expired/revoked grants fail;
- job-store round trip and daemon restart preserve the same immutable decision identity;
- the executor never calls a grant-building helper.

A manually constructed `ToolAuthorityGrant` passed directly to `verify_integrity()` is supplementary only.

## 6. Work package B — Canonical manifest and contract snapshot

### B1. Canonicalize the exact allowed contract set

At submission, resolve every requested tool through the production Broker catalog. Reject missing, direct-only, mutation-capable, schema-incomplete, or otherwise non-programmable contracts before job creation.

Create one canonical bounded snapshot containing at least:

- sorted tool names;
- stable implementation ID and version;
- caller policy;
- effect class;
- idempotency class;
- input schema digest;
- output schema digest;
- any contract revision used by the Broker.

Use deterministic serialization and SHA-256.

### B2. Persist and reuse one digest algorithm

Persist the snapshot or canonical reference plus digest with the grant/job. The same helper must compute the digest at:

1. submission;
2. executor admission;
3. each nested Broker invocation.

Do not hash an empty placeholder and later compare it with one concrete contract.

### B3. Verify tool membership and exact contract identity

The Broker must verify both:

- the invoked tool is a member of the frozen manifest; and
- the resolved production contract matches the frozen contract entry.

A tool name in an unrelated context list is not sufficient.

### B4. Tests

Extend `tests/tool_program_m014_authority_pipeline.rs` to prove through normal submission and execution:

- one valid read-only tool call succeeds;
- two valid tools use one deterministic catalog digest;
- reordered tool requests produce the same canonical digest;
- contract version, effect, caller policy, input schema, or output schema drift fails before invocation;
- an empty or placeholder contract snapshot is rejected;
- the underlying tool invocation counter remains zero on mismatch.

## 7. Work package C — Complete checkpoint and replay recovery

### C1. Define a resumable checkpoint schema

Extend `InterpreterCheckpoint` with a versioned bounded state sufficient to resume exactly at its saved point. Include as required by the current interpreter:

- program counter;
- bounded locals values, not only a hash;
- operand/control/loop frames if they exist outside locals;
- budget counters;
- next call sequence;
- completed calls or a durable reference to them;
- pending child wait identity and expected result slot;
- replay fingerprint version/reference;
- original absolute deadline in Unix milliseconds;
- checkpoint sequence and creation time;
- SHA-256 digest over the complete semantic checkpoint.

Use a deterministic representation. `DefaultHasher`, debug formatting, or process-randomized hashing is not an integrity mechanism.

### C2. Persist checkpoints atomically

Persist the checkpoint after completed-call commit and at explicit safe boundaries. Prefer SQLite transactional rows with monotonically increasing checkpoint sequence. If retaining files, use repository-standard cross-process locking, fsync/atomic replacement, and versioned records.

Never publish a newer checkpoint before the completed call or child identity it references is durable.

### C3. Load and restore before execution

In `src/scheduler/tool_program_executor.rs`:

1. load the latest valid checkpoint;
2. validate program/source/IR/grant/manifest/contract/workspace/backend/deadline identity;
3. restore the interpreter before the first resumed instruction;
4. reattach any pending child wait;
5. stop with a typed recoverable divergence if validation fails.

Do not load completed calls separately and then start at PC 0 when a later valid checkpoint exists. Do not set PC to a saved value without restoring the state that PC expects.

### C4. Original deadline authority

Persist the original absolute deadline at submission and include it in the grant/replay/checkpoint identity. On restart, compute remaining time from that absolute value. If elapsed, fail as timed out before execution. Never grant a fresh full timeout window.

### C5. Cross-process replay safety

Replace process-local-only journal locking with one of:

- SQLite transactional call reservation/completion/checkpoint rows with uniqueness and compare-and-set transitions; or
- an existing repository-standard inter-process lock plus append-only journal and atomic compaction.

The mechanism must tolerate an old daemon process still exiting while a replacement daemon starts. It must prevent lost reservations, completions, and checkpoints.

### C6. Tests

Create `tests/tool_program_m014_checkpoint_recovery.rs` proving:

- locals and control state resume with the same final output as uninterrupted execution;
- a completed call before checkpoint is not physically repeated;
- a pending child wait reattaches without resubmission;
- all budget counters continue from the checkpoint;
- next call sequence does not collide;
- original deadline remains authoritative;
- corrupt checkpoint digest fails closed;
- stale source/IR/grant/contract/path-policy/backend identity produces an inspectable divergence;
- two independent ledger/store instances cannot lose concurrent reservations or completions.

At least one test must use a program whose result is wrong if locals are not actually restored.

## 8. Work package D — Complete durable lineage and upgrade migration

### D1. Extend the typed lineage model

Add typed fields to `NewJob` and `JobRecord`, or a normalized `JobLineage` value, containing:

- parent program ID;
- parent job ID;
- parent attempt ID;
- canonical parent call ID;
- parent instruction sequence;
- relation kind;
- lineage creation timestamp where needed for audit/reconciliation.

Use a typed relation enum for at least Tool Program child execution.

### D2. Canonical call identity

Create the child lineage key from the actual interpreter call/child instruction identity, for example:

`program_id + parent_attempt_id + instruction_sequence + typed relation`

Do not derive `parent_call_id` from `request.op`, tool name, or operation debug formatting.

### D3. Add the next migration version

Add a new migration after the current repository schema version. It must:

- add every missing lineage column or normalized table;
- backfill safely where historical data permits;
- leave unknown historical lineage explicitly null rather than fabricated;
- add indexes for parent program, parent job, parent attempt, canonical call, sequence, and active descendants;
- be idempotent under the repository migration runner;
- upgrade a database already at the pre-M014 latest version.

Do not rely on editing migration v23 or any already-applied migration.

### D4. Preserve lineage through all transitions

Audit every `JobRecord { ..job }` update in both SQLite and in-memory stores. Remove explicit resets of lineage during:

- enqueue;
- begin attempt;
- mark running;
- request cancellation;
- block/unblock;
- retry;
- finish attempt;
- lost-worker recovery;
- daemon-generation recovery;
- terminalization.

Lineage is immutable after creation.

### D5. Tests

Create `tests/tool_program_m014_lineage_migration.rs` proving:

- a pre-M014 database upgrades and exposes all new columns/indexes;
- create/get/list/retry/cancel/block/recover/finish round-trip immutable lineage;
- identical operation types at different instruction sequences create distinct lineage identities;
- replay of the same instruction sequence finds the same child;
- historical null lineage remains null and does not crash reconciliation.

## 9. Work package E — Recursive scheduler-owned descendants

### E1. Recursive enumeration

Implement recursive active-descendant enumeration:

- SQLite: use a recursive CTE or bounded iterative query through indexed parent relationships;
- in-memory: use bounded breadth-first or depth-first traversal with a visited set.

Detect and report cycles rather than looping indefinitely.

### E2. Scheduler terminalization order

For cancellation, timeout, interruption, lost worker, and abandoned daemon generation:

1. persist parent terminalization/cancellation intent;
2. enumerate recursive active descendants;
3. request cancellation of descendant jobs and managed process groups;
4. wait for bounded reconciliation or persist unresolved descendants for the next scheduler reconciliation pass;
5. release permits and workspace leases only according to their canonical ownership lifecycle;
6. publish terminal completion with diagnostics if descendants remain unresolved.

The parent executor future may already have been dropped. The scheduler/store path must still work.

### E3. Restart reattachment

Before child submission, query by canonical lineage identity. If a queued/running/terminal child already exists:

- queued/running: reattach and wait;
- completed: consume its canonical typed result;
- failed/cancelled/timed-out: apply the defined child failure semantics;
- ambiguous duplicates: stop with a recoverable invariant failure.

### E4. Capacity-one behavior

Ensure a Tool Program waiting on a scheduler-owned child does not retain the resource permit needed by that child. Use the existing admission/resource model rather than adding unbounded bypasses.

### E5. Tests

Create `tests/tool_program_m014_recursive_descendants.rs` with real scheduler/store/executor behavior proving:

- child, grandchild, and deeper active descendants are enumerated and cancelled;
- cycles fail boundedly;
- parent timeout after executor future drop still cancels descendants;
- lost-worker and daemon-generation reconciliation cancels or reattaches descendants;
- active managed process groups terminate;
- attempts, process slots, permits, workspace leases, and running counters return to baseline;
- capacity-one parent/child execution completes without deadlock;
- restart reattaches an existing running child and does not submit a duplicate.

Assertions on `Duration`, field presence, or direct store counts without running scheduler behavior are insufficient.

## 10. Work package F — Fail-closed transactional notification delivery

### F1. Propagate persistence failures

Change notification creation and persistence APIs to return `Result`. Propagate serialization, SQL, constraint, transition, and transaction failures to the caller. If asynchronous callers cannot return directly, persist a typed failed-delivery state and surface it through inspection/health projections.

No closure-bearing path may only log a warning and continue as though persistence succeeded.

### F2. Add schema-level delivery identity

Use the next migration version to add bounded columns or a normalized delivery table for:

- injection key;
- injected event ID;
- claim owner and lease;
- injection-reserved timestamp;
- appended timestamp;
- acknowledged timestamp;
- retry count;
- last failure class/message.

Add a uniqueness constraint/index for injection key.

### F3. Idempotent parent-session append

Use the actual session store insertion boundary. The append must accept or derive the durable injection key and reject/reuse duplicates. When notification and session state share SQLite, prefer one transaction where repository architecture permits. Otherwise use a durable outbox/inbox protocol with uniqueness on both sides.

### F4. Correct digests

Replace every Tool Program notification MD5 payload identity with correctly labeled SHA-256. Add backward readers only where required; all new writes use the current version.

### F5. Recovery algorithm

Recovery must distinguish:

- pending, never claimed;
- claimed with live lease;
- expired claim;
- injection reserved but no session event;
- session event durably appended but not acknowledged;
- delivered/suppressed/terminally failed.

It must produce exactly one logical parent-session event and must not recreate terminal notifications.

### F6. Tests

Create `tests/tool_program_m014_notification_delivery.rs` using a real migrated database, the real session insertion API, two independent notification services, and separate SQLite pools. Prove:

- concurrent claim has one winner;
- database unavailable/locked/constraint failures do not report success;
- restart at every delivery boundary produces one durable session event;
- duplicate injection keys reuse/reject the existing event;
- delivered, suppressed, and terminally failed notifications are not recreated;
- new payload digests are 64-hex SHA-256 and match the canonical payload bytes.

## 11. Work package G — Canonical result and artifact integrity

### G1. Use the existing artifact abstraction

Route call, child, and large final output through the canonical workspace artifact store and/or RunStore. Do not construct filesystem paths and fabricate `ctx://` handles manually.

### G2. Typed artifact identity

Every retained artifact entry must contain:

- canonical handle;
- SHA-256 content digest;
- bounded preview;
- content length where available;
- producer identity: call ID/tool name or child job/attempt/run ID;
- terminal status;
- typed absence reason if content is intentionally not retained.

### G3. Child result convergence

Read child artifact/run identity from the child's canonical terminal result or RunStore record. Populate child job ID, attempt ID, run ID, artifact handles, and digests. Do not emit `None` for every artifact field after successful child execution.

### G4. Output spill behavior

When final output exceeds the inline bound:

1. persist through the canonical artifact store;
2. verify returned handle and digest;
3. store the handle/digest in the typed result;
4. keep only a bounded preview inline;
5. fail the Tool Program result persistence if artifact storage fails.

### G5. Complete semantic digest

Ensure the typed result digest authenticates all semantic fields, including artifact handles, artifact digests, producer identities, backend, attempt, result, and execution fingerprint/reference.

### G6. Tests

Create `tests/tool_program_m014_artifacts.rs` proving through real stores:

- call artifact handles resolve and match their digests;
- child artifact handles resolve and correlate to the child attempt/run;
- large final output spills through the canonical store;
- no manually fabricated handle resolves accidentally;
- artifact write failure fails closed;
- missing/corrupt artifact data produces bounded typed diagnostics;
- foreground, background notification, and inspection expose identical artifact identities.

## 12. Work package H — Real daemon process and failpoint harness

### H1. Locate the normal public boundary

Identify the current production daemon entrypoint and a supported public submission protocol used by clients. Prefer the existing socket, WebSocket, gRPC, or stdio protocol rather than adding a test-only alternate submission API.

Record the exact binary, command line, protocol request, and durable database/workspace paths in test comments and eventual closure evidence.

### H2. Add test-only failpoints

Add compile-time/test-feature-gated failpoints at minimum:

- after accepted permission decision but before job commit;
- after job/grant commit but before scheduler admission;
- after call reservation but before tool dispatch;
- after physical tool completion but before completed-call commit;
- after completed-call commit but before checkpoint commit;
- after checkpoint commit while waiting on a child;
- after notification claim;
- after injection reservation;
- after durable parent-session append but before acknowledgement;
- after artifact write but before typed result commit.

Failpoints must be unavailable in normal production builds.

### H3. Daemon harness behavior

Create `tests/tool_program_m014_daemon_recovery.rs` or an equivalent repository-standard process test that:

- builds or locates the actual daemon binary;
- starts it with isolated temporary config, workspace, SQLite database, socket/port, and artifact directories;
- waits for readiness through the public protocol;
- submits a Tool Program through that protocol;
- activates one failpoint;
- kills the daemon process without graceful executor cleanup;
- confirms the old process and managed child process group are gone or boundedly terminating;
- restarts a fresh daemon against the same state;
- observes terminal state, result, notification, artifacts, descendants, permits, and inspection through public/durable boundaries.

Do not share in-memory objects across restart phases.

### H4. Required scenarios

At minimum test:

1. crash before job commit creates no runnable orphan;
2. crash after job/grant commit resumes with the same decision identity;
3. crash after call reservation does not create an ambiguous duplicate call;
4. crash after physical completion before completion commit follows the explicit at-least-once/idempotency policy and never falsely claims exactly-once;
5. crash after completion commit does not physically re-execute the call;
6. crash after checkpoint commit restores locals/control state and original deadline;
7. crash while waiting on a child reattaches the existing child;
8. parent timeout terminates recursive child process groups;
9. crash after notification claim recovers delivery;
10. crash after session append before acknowledgement produces one event;
11. crash after artifact write before result commit reconciles or safely cleans the artifact;
12. corrupted checkpoint/result/artifact fails closed with inspectable diagnostics.

### H5. CI and platform handling

The core process tests must run on the repository's primary supported CI platform. Platform-specific process-group assertions may be gated, but an ungated process-level restart suite must remain. Skipping all daemon tests locally or in CI is not closure.

## 13. Work package I — Governance and documentation reconciliation

After implementation and all required tests pass:

- change this plan status from `ready for handoff` to `closing`;
- update `plans/registry.md` to show M014 closing;
- update `plans/subsystems/tool-programs-correctness-closure-addendum.md` with factual implementation status only;
- update the canonical Tool Programs roadmap status/dependency graph to include M014;
- update architecture documentation to describe only implemented mechanisms;
- retain M013 as a historical conditionally closed implementation record;
- correct M013's status record without erasing its original claims from Git history;
- do not create or accept `plans/closure/tool-programs/014-status.md` in the implementation pass.

An independent reviewer must inspect production code, execute or verify the evidence, and create the M014 closure record in a separate commit.

## 14. Required tests and commands

Run with repository resource constraints respected.

```bash
cargo fmt --all -- --check
cargo check -p codegg --all-targets
cargo test --test tool_program_m014_authority_pipeline -- --test-threads=1
cargo test --test tool_program_m014_checkpoint_recovery -- --test-threads=1
cargo test --test tool_program_m014_lineage_migration -- --test-threads=1
cargo test --test tool_program_m014_recursive_descendants -- --test-threads=1
cargo test --test tool_program_m014_notification_delivery -- --test-threads=1
cargo test --test tool_program_m014_artifacts -- --test-threads=1
cargo test --test tool_program_m014_daemon_recovery -- --test-threads=1
cargo test --test tool_broker_integration -- --test-threads=1
cargo test --test tool_program_m013_authority -- --test-threads=1
cargo test --test tool_program_m013_notifications_sqlite -- --test-threads=1
cargo test --test tool_program_m013_lineage -- --test-threads=1
cargo test --test tool_program_m013_descendants -- --test-threads=1
cargo test --test tool_program_m013_replay -- --test-threads=1
cargo test --test tool_program_m013_results -- --test-threads=1
cargo test --test scheduler_cancellation -- --test-threads=1
cargo test --test scheduler_restart_recovery -- --test-threads=1
cargo test --test scheduler_contention -- --test-threads=1
cargo test --test scheduler_permit_lifecycle -- --test-threads=1
cargo test --test managed_process_descendants -- --test-threads=1
cargo test --test tool_program_fault_injection -- --test-threads=1
cargo test --test tool_program_runtime -- --test-threads=1
```

Then run the repository-standard bounded workspace suite. List every skipped or failing test with the exact reason and explain why it cannot invalidate M014. Do not claim GitHub CI evidence unless a workflow run is attached to the reviewed commit.

## 15. Binary closure criteria

M014 may move from `closing` to independently reviewed closure only when every criterion below is true.

### Authority and contracts

- **C-01**: The grant is created from the actual accepted direct-call permission/path-policy decision.
- **C-02**: Denied, missing, synthetic, or identity-only decision material creates no Tool Program job.
- **C-03**: The immutable decision identity and grant survive job-store round trip and daemon restart.
- **C-04**: The executor never creates, repairs, substitutes, or widens a grant.
- **C-05**: Grant integrity covers every security-relevant decision and program-binding field.
- **C-06**: Expired, revoked, stale-policy, stale-path-policy, malformed, or tampered grants fail before tool invocation.
- **C-07**: Submission freezes the exact canonical allowed contract snapshot.
- **C-08**: Submission, executor, and Broker use one canonical contract digest algorithm.
- **C-09**: A normal authorized nested read-only call succeeds through the production path.
- **C-10**: Tool membership or contract drift fails before invocation and creates no completed-call record.

### Checkpoint, replay, and deadline

- **C-11**: The executor loads the latest valid checkpoint and invokes restore before resumed execution.
- **C-12**: Checkpoint state contains and restores bounded locals and every required control frame.
- **C-13**: Budget counters and next call sequence continue from the checkpoint without reset or collision.
- **C-14**: Pending child wait identity is persisted and restored.
- **C-15**: A resumed program produces the same result as uninterrupted execution in a state-sensitive test.
- **C-16**: A durably completed call is never physically repeated after restart.
- **C-17**: The original absolute deadline is persisted, fingerprinted, restored, and authoritative.
- **C-18**: Checkpoint/replay corruption or identity drift stops with a typed inspectable divergence.
- **C-19**: Concurrent or overlapping process writers cannot lose, tear, or overwrite call/checkpoint state.
- **C-20**: New checkpoint, replay, and notification integrity records use correctly labeled SHA-256.

### Lineage and descendants

- **C-21**: Typed lineage includes parent program, job, attempt, canonical call ID, instruction sequence, and relation kind.
- **C-22**: A new migration upgrades a database already at the pre-M014 latest version.
- **C-23**: Every JobStore create/read/update/retry/cancel/block/recover/finish path preserves immutable lineage.
- **C-24**: Canonical child identity is derived from actual parent execution identity and sequence, not operation name.
- **C-25**: Replay of one child instruction reuses one child; distinct sequences create distinct children.
- **C-26**: The scheduler can enumerate recursive active descendants without payload scanning.
- **C-27**: Parent cancellation, timeout, interruption, lost worker, and daemon-generation abandonment reconcile recursive descendants after the executor future is gone.
- **C-28**: Restart reattaches existing queued/running children and consumes terminal children without duplicate submission.
- **C-29**: Capacity-one parent/child execution completes without deadlock.
- **C-30**: Descendant jobs, attempts, process groups, permits, workspace leases, and counters converge to baseline or an explicit recoverable unresolved state.

### Notification delivery

- **C-31**: Notification creation and persistence return or durably record serialization/SQL/constraint failures.
- **C-32**: SQLite compare-and-set is authoritative for claim, injection reservation, acknowledgement, suppression, failure, and lease recovery.
- **C-33**: Injection key is schema-level durable and unique.
- **C-34**: Parent-session insertion is idempotent through the injection key.
- **C-35**: Two independent services and pools cannot both claim or append the same logical notification.
- **C-36**: Restart at each delivery boundary produces exactly one durable parent-session event.
- **C-37**: Delivered, suppressed, and terminally failed notifications are not recreated.
- **C-38**: New notification payload digests are correct SHA-256 values.

### Results and artifacts

- **C-39**: Call artifacts use canonical resolvable handles and verified content digests.
- **C-40**: Child artifacts include real attempt/run identity, canonical handles, and verified digests, or a typed absence reason.
- **C-41**: Large final output is persisted through the canonical artifact store and fails closed on storage failure.
- **C-42**: Foreground, background notification, and inspection expose one authoritative typed result and identical artifact identities.
- **C-43**: Result integrity covers every semantic result and artifact field.
- **C-44**: Missing or corrupt result/artifact data fails closed with bounded diagnostics.

### Process evidence and governance

- **C-45**: A real daemon process accepts a Tool Program through a public protocol boundary.
- **C-46**: Tests kill the daemon at deterministic failpoints and restart a fresh process against the same state.
- **C-47**: Restart tests share no in-memory service, scheduler, ledger, or cache objects.
- **C-48**: Process tests cover completed-call replay, checkpoint restore, child reattachment, notification append-before-ack, artifact/result commit, and process-group cleanup.
- **C-49**: Required process tests run on the primary CI platform and are not universally ignored.
- **C-50**: Full targeted formatting, compilation, migrations, static guards, and tests pass.
- **C-51**: No unresolved high or medium authorization, contract, recovery, lineage, notification, artifact, resource, process, or evidence finding remains.
- **C-52**: Registry, roadmap/addendum, architecture docs, implementation plan, commit SHAs, CI/test evidence, and M014 closure record agree.
- **C-53**: The implementation pass did not create or accept the M014 closure record.
- **C-54**: An independent reviewer accepts `plans/closure/tool-programs/014-status.md` in a separate commit after inspecting production behavior and evidence.

## 16. Explicit rejection examples

The following do not satisfy M014:

- hashing program/workspace/session strings and naming the result an authorization decision;
- treating `permission_mode` or a non-empty path-policy string as proof that permission was accepted;
- hashing an empty contract list at submission and one concrete contract at invocation;
- manually constructing grants in tests without running the production permission/submission path;
- adding `restore_checkpoint()` without calling it from the executor;
- setting PC to a saved value while locals/control state remain default;
- storing only a locals hash and claiming locals were restored;
- setting `original_deadline_millis` to `None` in production and claiming deadline recovery;
- adding lineage fields to comments/tests without the next migration version;
- clearing lineage in `JobRecord { ..job }` transitions;
- cancelling direct children only and calling it recursive convergence;
- generating `parent_call_id` from an operation enum or tool name;
- returning child artifacts whose handle, run ID, and digest are all absent;
- writing output directly to a path and fabricating a `ctx://` handle;
- warning on artifact or notification persistence failure and returning success;
- using MD5 for a field documented as SHA-256;
- using a process-local mutex as proof of cross-process replay safety;
- reconstructing stores inside one test process and calling it daemon restart;
- marking a mandatory binary criterion deferred while declaring the milestone closed;
- creating and self-accepting the closure record in the implementation commit.

## 17. Final handoff condition

The implementation model's final response must provide:

- ordered commit SHAs matching the required sequence;
- files and migration versions changed per work package;
- the exact production permission decision type and path reused;
- the exact canonical contract snapshot format and digest helper;
- the exact checkpoint schema and restoration call site;
- the exact lineage migration version and recursive query/cancellation mechanism;
- the exact session idempotency and artifact-store APIs used;
- daemon command, public protocol, failpoints, and kill/restart scenarios;
- exact tests and commands executed with pass/fail counts;
- any skipped test with justification;
- remaining findings, if any;
- confirmation that M014 was moved only to `closing`;
- confirmation that no M014 closure record was created or accepted.

If any C-01 through C-53 criterion is not satisfied, leave M014 `active` or `closing` and state the blocker precisely.
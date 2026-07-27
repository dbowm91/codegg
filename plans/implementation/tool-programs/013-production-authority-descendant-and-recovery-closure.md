# Tool Programs Milestone 013 — Production Authority, Descendant, Delivery, and Recovery Closure

Status: closing

Class: corrective implementation / authorization / durable ownership / scheduler convergence / recovery / evidence closure

Baseline reviewed:

- `d056e4236e1ef10b4639b8bbf05557090dc6112c`

Predecessor records:

- `plans/implementation/tool-programs/012-authority-recovery-and-delivery-corrective-closure.md`
- `plans/closure/tool-programs/012-status.md`
- `plans/subsystems/tool-programs-correctness-closure-addendum.md`

Target closure record:

- `plans/closure/tool-programs/013-status.md`

## 1. Objective

Close the remaining Tool Programs production-boundary defects after M012 without redesigning the subsystem or expanding the programmable tool palette.

M013 is complete only when the production daemon path provides all of the following:

1. a versioned authority grant derived from a real permission and workspace path-policy decision before job admission;
2. durable persistence of that grant or an integrity-protected reference sufficient for exact reconstruction and revocation checks;
3. Broker verification of principal, workspace, path policy, caller class, effect class, manifest, contract version, policy revision, validity interval, and revocation state on every programmatic nested call;
4. SQLite-authoritative notification claim, injection, acknowledgement, suppression, failure, lease recovery, and restart behavior across independent service instances;
5. durable child lineage stored and queryable by the scheduler, including parent program, job, attempt, call identity, and instruction sequence;
6. scheduler-owned recursive descendant cancellation and reconciliation that does not depend on the parent executor future remaining alive;
7. restart reattachment to existing child jobs and completed calls without duplicate physical execution;
8. checkpoint restoration and full replay fingerprint validation;
9. concurrency-safe, integrity-checked durable replay state without mislabeled or weak digests;
10. one integrity-protected typed result containing real resolvable call, child, and output artifact handles;
11. process-level evidence through the normal daemon, scheduler, SQLite, and artifact boundaries;
12. closure documentation whose commit identities and claims match the repository state.

## 2. Handoff profile for a smaller implementation model

This plan is deliberately prescriptive.

### 2.1 Required execution style

- Work in the ordered work-package sequence below.
- Keep each work package in a separate commit unless a compile-only mechanical update must accompany the same schema change.
- Add failing tests before changing production behavior.
- Do not mark the milestone closed. Move the plan to `closing` only after all implementation and required tests pass.
- Do not create or approve `plans/closure/tool-programs/013-status.md`; closure review is a separate task.
- Do not use comments, type existence, static string search, or hand-constructed values as proof of runtime behavior.
- Do not weaken an acceptance criterion because the current test harness lacks a seam. Add the narrow seam or harness needed to exercise the production boundary.
- Preserve existing public behavior unless this plan explicitly changes it.

### 2.2 Non-goals

Do not:

- redesign the restricted Python parser, compiler, IR, or interpreter instruction set;
- add mutation-capable, destructive, approval-sensitive, shell, patch, Git mutation, commit, push, or subagent tools to the programmable palette;
- implement hosted Tool Programs;
- redesign the provider registry or normal direct tool UX;
- replace the scheduler, job store, session store, RunStore, or artifact store wholesale;
- introduce a second durable authority or notification database;
- add a process-local cache as an authoritative source of truth;
- claim exactly-once behavior without an idempotent durable injection key and restart test;
- swallow storage, migration, transition, cancellation, replay, artifact, or digest errors.

### 2.3 Default architecture decisions

Use these decisions unless existing production abstractions require a smaller equivalent:

- SQLite is authoritative for jobs, lineage, notification state, delivery identity, and restart recovery.
- The authority grant is created before Tool Program job submission from the same permission and workspace path-policy decision used to authorize the direct `tool_program` invocation.
- The submitted job carries the immutable grant or a durable grant reference plus digest. The executor must not fabricate a replacement grant.
- The scheduler owns descendant enumeration and cancellation through persisted lineage.
- Replay identity is a versioned fingerprint record, not an ad hoc collection of optional comparisons.
- SHA-256 is the minimum digest algorithm for newly written Tool Program integrity records. Never label MD5 output as SHA-256.
- Production remains `native_only`; unreachable hosted policies are rejected before submission.

## 3. Verified baseline findings

The M012 post-implementation audit found the following unresolved defects.

### F01 — Authority grant remains synthetic

`to_core_context` and `build_authority_grant` derive identity and decision material from program, workspace, session, agent, source, and timestamp strings. The executor creates the grant after admission. This is not the persisted result of a real permission/path-policy decision.

### F02 — Broker does not verify grant scope

The Broker rejects `Unverified` but does not validate the grant's digest, expiry, revocation, principal, workspace, path-policy identity, caller class, effect class, manifest, contract version, or policy revision against the invocation.

### F03 — Notification SQLite CAS path is incorrect and unproven

The transition SQL uses invalid or inconsistent positional parameter handling and JSON-serialized state strings while SQL branches compare bare values. Tests use an in-memory service or two `Arc` references to one instance rather than independent services sharing one SQLite database.

### F04 — Child lineage is not durable

Child `NewJob` fields are populated but job stores discard them, SQLite has no corresponding persisted columns/query mapping, and `parent_call_id` is derived from the operation name rather than a canonical call identity plus sequence.

### F05 — Scheduler does not own descendant cancellation

Scheduler timeout drops the executor future and cancels its token, while descendant cancellation remains inside the dropped parent future. There is no persisted descendant enumeration, recursive cancellation, restart reconciliation, or lost-worker cleanup based on lineage.

### F06 — Recovery and replay identity remain incomplete

The executor reloads completed calls but not the durable checkpoint. Replay compares only sequence, tool name, and input. Authority, context, contract, manifest, source/IR, workspace/path-policy, backend, deadline, call order, and child identity are not bound into one verified fingerprint.

### F07 — Replay journal is not concurrency-safe or correctly integrity-labeled

The journal uses whole-file read/modify/rename without locking or transaction ownership. Raw call request/result bodies are retained despite redaction-oriented comments. Helper functions label MD5 output as `sha256`.

### F08 — Result and artifact convergence remains partial

Child results still return empty artifact arrays. Persisted `child_artifacts` remain empty. Call artifact digests may be absent. The result digest authenticates only `ProgramResult`, not the complete typed result record.

### F09 — Process-level evidence is absent

The process-recovery tests do not start a daemon, kill a process, use failpoints, share SQLite across independent service instances, reattach children, or verify process-group and permit convergence.

### F10 — Governance evidence is inconsistent

The M012 closure record names the plan-registration commit as its implementation commit, marks criteria as passing while admitting their production mechanism is absent, and records test claims without attached repository CI evidence.

## 4. Required commit sequence

Use this sequence so review can isolate regressions:

1. `test(tool-programs): add failing M013 authority and broker verification coverage`
2. `fix(tool-programs): persist and verify production authority grants`
3. `test(tool-programs): add SQLite notification transition and restart coverage`
4. `fix(tool-programs): make notification lifecycle SQLite authoritative`
5. `test(jobs): add durable lineage migration and round-trip coverage`
6. `fix(jobs): persist Tool Program descendant lineage`
7. `test(scheduler): add descendant cancellation and reattachment coverage`
8. `fix(scheduler): own descendant cancellation and reconciliation`
9. `test(tool-programs): add checkpoint and replay fingerprint coverage`
10. `fix(tool-programs): restore checkpoints and enforce replay fingerprints`
11. `fix(tool-programs): make replay journal concurrency-safe and integrity-correct`
12. `test(tool-programs): add complete result and artifact integrity coverage`
13. `fix(tool-programs): converge typed results and real artifacts`
14. `test(tool-programs): add daemon process and failpoint recovery harness`
15. `docs(plans): move Tool Programs M013 to closing`

Do not squash these into one implementation commit before review.

## 5. Work package A — Production authority decision and durable grant

### A1. Inspect the real permission boundary

Start with:

- `src/agent/loop.rs`
- `src/tool/broker.rs`
- `src/tool/tool_program.rs`
- `src/tool/tool_program_context.rs`
- `src/tool/backend.rs`
- existing permission/path-policy types and ADR-0001

Identify the exact production decision object or decision outputs used when the direct `tool_program` call is authorized. Reuse that boundary. Do not create a second permissive policy evaluator.

### A2. Create a versioned immutable grant

The persisted grant must contain bounded non-secret identity sufficient to verify:

- grant schema version;
- grant ID;
- principal identity;
- workspace ID;
- canonical workspace path-policy ID or revision;
- session, turn, and agent identity where applicable;
- permission mode and policy revision;
- caller class allowed;
- maximum effect class allowed;
- exact allowed tool manifest digest;
- contract catalog/version digest;
- source and IR identity or a submission fingerprint that binds them;
- issued-at, expiry, and revocation reference/state;
- decision outcome digest;
- grant digest over every security-relevant field.

Use typed enums for caller/effect classes where existing types permit it. Avoid security decisions based on free-form strings.

### A3. Persist before admission

The grant must be created before `NewJob` submission and persisted in the Tool Program payload or a canonical durable grant table/reference.

The executor must deserialize and verify the persisted grant. It must not call a helper that creates a new grant from execution context.

### A4. Fail closed

Submission or execution must fail when:

- the permission decision is unavailable;
- the path-policy decision cannot be identified;
- required grant fields are empty;
- the grant digest does not match;
- the grant has expired or is revoked;
- the workspace/path-policy revision differs;
- the persisted manifest or contract digest differs;
- the program source or IR identity differs from the submission fingerprint.

### A5. Tests

Create `tests/tool_program_m013_authority.rs` using production constructors and submission paths. It must prove:

- a real accepted permission decision creates the persisted grant;
- no executor-side synthetic grant is accepted;
- tampering each security-relevant field fails;
- stale, expired, and revoked grants fail;
- workspace, path-policy, principal, caller, effect, manifest, and contract mismatches fail;
- the grant survives job-store round trip and daemon reconstruction.

A test that manually constructs a `ToolAuthorityGrant` and calls `is_valid()` is insufficient.

## 6. Work package B — Broker scope verification

### B1. Add one verification function

Add a single Broker-owned verification path that receives:

- the persisted grant;
- the actual `BrokerInvocationContext`;
- the resolved `ToolContract`;
- the frozen Tool Program manifest/contract snapshot;
- current policy/path-policy revision and revocation view;
- current time.

It must verify every field before tool execution.

### B2. Effect and caller checks

The grant's allowed caller class and maximum effect class must be checked against the actual caller and resolved contract. A read-only grant must never authorize an approval-sensitive or mutation-capable contract even if the tool name appears in a malformed manifest.

### B3. Terminal behavior

All verification failures must produce typed denied/validation outcomes. They must never increment completed-call counters or enter the completed-call replay ledger.

### B4. Tests

Extend `tests/tool_program_m013_authority.rs` and `tests/tool_program_m012_broker_failures.rs` with real Broker execution tests. Prove that the underlying tool implementation is not invoked on any scope mismatch.

## 7. Work package C — SQLite-authoritative notification lifecycle

### C1. Correct the storage representation

Use one canonical textual state representation in SQLite. Do not compare JSON-quoted enum strings with bare SQL values.

Use named parameters or valid SQLite positional parameters. Add database constraints where practical.

### C2. Authoritative transitions

Implement transactional compare-and-set operations for:

- pending -> claimed;
- claimed -> delivered;
- pending/claimed -> suppressed;
- pending/claimed -> failed;
- expired claim -> pending or expired according to policy;
- durable injection reservation and completion.

The database row count and transaction outcome are authoritative. The in-memory cache updates only after commit.

### C3. Durable injection identity

Persist an injection key with a uniqueness constraint. The parent-session append must be idempotent through that key. The system must distinguish:

1. claimed but not injected;
2. injection reserved;
3. durable session event appended;
4. acknowledgement committed.

Recovery must inspect durable state and finish exactly one logical delivery.

### C4. Error propagation

Serialization, SQL, transaction, constraint, append, and acknowledgement failures must be returned or recorded as failed state. Logging a warning and returning success is forbidden.

### C5. Tests

Create `tests/tool_program_m013_notifications_sqlite.rs` that uses:

- a real migrated SQLite database;
- two independently constructed notification service instances;
- separate connections/pools pointing to the same database;
- concurrent claim tasks;
- explicit restart by dropping both services and constructing new instances;
- failpoints before claim commit, after claim, before append, after durable append, and before acknowledgement.

Prove exactly one durable parent-session event after every restart point.

## 8. Work package D — Durable descendant lineage schema

### D1. Add migration

Add a schema migration for nullable job columns or a normalized lineage table containing at least:

- child job ID;
- parent program ID;
- parent job ID;
- parent attempt ID;
- canonical parent call ID;
- parent instruction sequence;
- relation kind;
- created timestamp.

Add indexes for parent job, parent attempt, parent call, and active descendant queries.

### D2. Store round trip

Update:

- `NewJob` and `JobRecord` as needed;
- `InMemoryJobStore`;
- `SqliteJobStore` insert/select/row mapping;
- protocol conversion and submission constructors;
- retry/requeue paths so lineage is preserved, not reset to `None`.

### D3. Canonical call identity

Use the interpreter's durable call ID and instruction sequence. Do not derive parent-call identity from operation name.

### D4. Query API

Add a bounded JobStore query for direct children and active descendants. The scheduler must not scan arbitrary payload JSON.

### D5. Tests

Create `tests/tool_program_m013_lineage.rs` proving:

- SQLite round trip retains all lineage fields;
- retry and restart retain lineage;
- identical child operations at different instruction sequences create distinct child identities;
- replay of the same sequence resolves the existing child;
- lineage query returns only the correct descendants.

## 9. Work package E — Scheduler-owned descendant cancellation and reattachment

### E1. Cancellation ownership

When a parent job or attempt is cancelled, timed out, interrupted, abandoned by daemon-generation reconciliation, or declared lost-worker, the scheduler must:

1. persist parent terminal/cancellation intent;
2. enumerate active descendants from durable lineage;
3. request cancellation for every descendant;
4. recursively repeat for nested descendants;
5. wait or reconcile until descendant jobs/attempts and managed process groups are terminal;
6. release permits and workspace leases;
7. only then declare convergence complete or record a bounded unresolved cleanup failure.

This cannot rely on code inside the parent executor future.

### E2. Timeout ordering

Do not drop the executor future and immediately publish parent completion before descendant cancellation ownership is transferred. Introduce a scheduler cleanup phase or equivalent typed state.

### E3. Restart reattachment

On interpreter restart, a child instruction must look up the durable child identity. If an existing child is queued/running, reattach and wait. If terminal, consume its typed result. Do not submit a duplicate.

### E4. Capacity-one behavior

A Tool Program waiting for build/test child work must not hold a conflicting process/build/test permit that prevents the child from running. Adjust Tool Program resource dimensions or split coordinator versus child resources narrowly.

### E5. Tests

Create `tests/tool_program_m013_descendants.rs` using the real scheduler and SQLite store. Prove:

- scheduler timeout cancels an active child even when the parent executor future is aborted;
- explicit parent cancellation cancels descendants recursively;
- lost-worker and daemon-generation reconciliation cancel descendants;
- restart reattaches to an existing child;
- capacity-one execution completes without deadlock;
- job, attempt, process-group, permit, and workspace-lease counts return to baseline.

## 10. Work package F — Checkpoint restoration and replay fingerprint

### F1. Versioned replay identity

Define one versioned replay fingerprint that binds:

- program ID and invocation identity;
- authority grant digest;
- execution-context digest;
- workspace and path-policy revision;
- frozen tool manifest digest;
- contract catalog/version digest;
- source digest;
- IR digest;
- backend selection;
- original absolute deadline;
- call sequence and call ID;
- tool name and contract identity;
- normalized input digest;
- child submission identity where applicable;
- control-flow/checkpoint identity needed to prove the replayed call belongs at that instruction.

### F2. Restore checkpoint

Load and validate the latest durable checkpoint before execution. Restore the interpreter program counter, loop/control state, locals or bounded state required by the existing checkpoint type, budgets, iteration counters, completed calls, and pending child wait state.

Do not merely start from instruction zero with a completed-call map unless the full fingerprint proves deterministic equivalence and all state is reconstructed.

### F3. Deadline authority

Persist the original absolute deadline at submission or first execution and reuse it after restart. Never reset the full timeout window.

### F4. Divergence behavior

On mismatch:

- do not execute the disputed call;
- persist a typed recoverable result and divergence record;
- expose expected versus observed fingerprint components in bounded diagnostic form;
- stop the attempt safely.

### F5. Tests

Create `tests/tool_program_m013_replay.rs` proving:

- completed calls are not physically re-executed after process restart;
- checkpoint state resumes at the correct instruction;
- each fingerprint field independently causes fail-closed divergence when changed;
- original deadline remains authoritative;
- pending child wait reattaches rather than resubmits.

Use an invocation-counting test tool behind the production Broker to prove no physical duplicate execution.

## 11. Work package G — Concurrency-safe replay storage and integrity

### G1. Storage authority

Replace unsafe concurrent whole-file read/modify/write behavior with one of:

- SQLite transactional journal rows; or
- an append-only, locked, versioned journal with atomic compaction and explicit single-writer ownership.

Prefer SQLite if it can reuse the daemon database without creating cross-store cycles.

### G2. Redaction and bounds

Persist only the minimum replay material needed for deterministic recovery. Raw secrets and unrestricted tool bodies must not enter inspection-oriented records. If exact normalized input is required for execution replay, store it in the protected execution journal and keep public inspection projections redacted.

### G3. Digests

Use SHA-256 consistently for new integrity records. Correct every helper that emits `sha256:` while computing MD5. Migrations or backward-compatible readers may recognize legacy records but new writes must be correct and versioned.

### G4. Tests

Add concurrent reservation/completion tests with independent writers. Prove no lost update, torn record, silent overwrite, or cross-program corruption.

## 12. Work package H — Complete typed result and artifact integrity

### H1. Full-record digest

The persisted result digest must cover every semantic field:

- schema version;
- program and attempt identity;
- selected backend;
- terminal result;
- call artifacts;
- child artifacts;
- output artifact;
- any execution fingerprint or lineage references required by consumers.

Exclude only the digest field itself and explicitly non-semantic recording metadata.

### H2. Real call artifacts

Each call artifact must contain:

- resolvable artifact handle where output is retained;
- SHA-256 content digest;
- bounded preview;
- tool and call identity;
- success/failure status.

If output is intentionally not retained, encode a typed absence reason rather than an unexplained `None`.

### H3. Real child artifacts

Consume the child's canonical typed result or RunStore artifact references. Populate child job ID, attempt/run ID, terminal status, artifact handle, and digest. Do not return `artifacts: vec![]` unconditionally.

### H4. Output artifact

Large final output must spill through the existing artifact store and return a bounded handle. Foreground, background notification, and inspection must all read the same persisted result record.

### H5. Tests

Create `tests/tool_program_m013_results.rs` proving:

- tampering any semantic result field causes digest failure;
- call, child, and output handles resolve and match their digests;
- foreground, background notification, and inspection return the same status, digest, and artifact identities;
- missing or corrupt artifacts fail closed with inspectable diagnostics.

## 13. Work package I — Production native-only truthfulness

Retain the model-facing `native_only` enum.

Also reject any non-native backend policy carried through an internal execution context before submission. Do not retain a silent hosted-to-native fallback path in production Tool Program execution.

Provider hosted adapters may remain library/experimental code, but they must not be described as production Tool Program execution until a later independently planned integration.

Add a test that passes every legacy hosted policy through the normal tool execution path and proves rejection before scheduler submission.

## 14. Work package J — Process-level closure harness

### J1. Daemon harness

Add a test harness that can:

- start the real daemon with a temporary workspace and SQLite database;
- submit a Tool Program through a public protocol boundary;
- observe durable job/attempt/notification/result state;
- activate named test-only failpoints;
- kill the daemon process without graceful cleanup;
- restart against the same workspace and database;
- wait for bounded reconciliation.

Failpoints must be compile-time/test gated and unavailable in normal production builds.

### J2. Required process scenarios

At minimum prove:

1. crash after call reservation but before dispatch;
2. crash after physical tool completion but before completed-call commit;
3. crash after completed-call commit but before checkpoint commit;
4. crash while waiting on a running child;
5. crash after notification claim;
6. crash after durable parent-session injection but before acknowledgement;
7. parent scheduler timeout while child process group is active;
8. result or artifact corruption before inspection.

### J3. Independent service concurrency

The notification concurrency test must use separate service objects and separate SQLite connections. The process test must not share the same in-memory maps across the simulated restart.

### J4. Evidence

Record exact commands, test counts, relevant logs, and commit SHA in the eventual M013 closure record. Do not claim GitHub CI evidence unless a run is actually attached to the reviewed commit.

## 15. Work package K — Documentation and governance reconciliation

After implementation and tests pass:

- change this plan status from `ready for handoff` to `closing`;
- update `plans/registry.md` to show M013 closing;
- update `plans/subsystems/tool-programs-correctness-closure-addendum.md` only for factual implementation status;
- update architecture docs to describe the implemented authority, notification, lineage, recovery, result, and native-only boundaries;
- leave M012 as a historical conditional implementation record;
- do not rewrite M012 history to imply it satisfied M013 mechanisms;
- do not create or accept `plans/closure/tool-programs/013-status.md` in the implementation pass.

## 16. Required tests and commands

Run with repository resource constraints respected.

```bash
cargo fmt --all -- --check
cargo check -p codegg --all-targets
cargo test --test tool_program_m013_authority -- --test-threads=1
cargo test --test tool_program_m013_notifications_sqlite -- --test-threads=1
cargo test --test tool_program_m013_lineage -- --test-threads=1
cargo test --test tool_program_m013_descendants -- --test-threads=1
cargo test --test tool_program_m013_replay -- --test-threads=1
cargo test --test tool_program_m013_results -- --test-threads=1
cargo test --test tool_program_m013_process_recovery -- --test-threads=1
cargo test --test tool_broker_integration -- --test-threads=1
cargo test --test tool_program_notifications -- --test-threads=1
cargo test --test scheduler_cancellation -- --test-threads=1
cargo test --test scheduler_restart_recovery -- --test-threads=1
cargo test --test scheduler_contention -- --test-threads=1
cargo test --test scheduler_permit_lifecycle -- --test-threads=1
cargo test --test managed_process_descendants -- --test-threads=1
cargo test --test tool_program_fault_injection -- --test-threads=1
cargo test --test tool_program_runtime -- --test-threads=1
```

Then run the repository-standard bounded workspace suite. Preserve the intentional low build concurrency documented by the repository. Any skipped or failing test must be listed with an exact reason and an explanation of why it cannot invalidate M013.

## 17. Binary closure criteria

M013 may move from `closing` to independently reviewed closure only when all criteria below are true.

### Authority and Broker

- **C-01**: The authority grant is created from the actual accepted permission/path-policy decision before Tool Program job submission.
- **C-02**: The immutable grant or canonical durable grant reference is persisted with the job and survives SQLite round trip and daemon restart.
- **C-03**: The executor never fabricates or substitutes a new grant.
- **C-04**: Grant integrity covers every security-relevant field and is verified before execution.
- **C-05**: Missing, malformed, expired, revoked, stale-revision, or tampered grants fail closed.
- **C-06**: Every nested Broker call verifies principal, workspace, path policy, caller class, effect class, manifest, contract version, and policy revision.
- **C-07**: A grant cannot authorize a contract with a stronger effect class than allowed.
- **C-08**: Authority failure invokes no underlying tool and creates no completed-call record.

### Notification delivery

- **C-09**: SQLite compare-and-set is authoritative for all notification state transitions.
- **C-10**: Two independent services sharing one database cannot both claim the same notification.
- **C-11**: SQL, serialization, transaction, append, and acknowledgement errors never report success.
- **C-12**: Injection identity is durable and unique.
- **C-13**: Restart before claim, after claim, after injection reservation, after durable append, and before acknowledgement yields exactly one parent-session event.
- **C-14**: Delivered, suppressed, and terminally failed notifications are not recreated by recovery.

### Descendant ownership

- **C-15**: Child lineage is persisted in SQLite and round-trips through every job-store path.
- **C-16**: Lineage includes parent program, job, attempt, canonical call ID, and instruction sequence.
- **C-17**: Identical operations at different sequences create distinct children; replay of one sequence reuses one child.
- **C-18**: The scheduler can enumerate direct and recursive active descendants without payload scanning.
- **C-19**: Parent cancellation, timeout, interruption, lost-worker reconciliation, and daemon-generation abandonment cancel descendants independently of the executor future.
- **C-20**: Restart reattaches to queued/running children and consumes terminal child results without duplicate submission.
- **C-21**: Capacity-one child execution completes without deadlock.
- **C-22**: Descendant jobs, attempts, process groups, permits, and workspace leases converge to baseline after cancellation or timeout.

### Replay and recovery

- **C-23**: The latest valid checkpoint is restored before execution resumes.
- **C-24**: Replay fingerprint binds authority, context, workspace/path policy, manifest, contract, source, IR, backend, deadline, call order, call ID, sequence, tool, input, and child identity where applicable.
- **C-25**: A durably completed call is never physically re-executed after restart.
- **C-26**: A pending child wait reattaches and does not resubmit.
- **C-27**: The original absolute deadline remains authoritative across restart.
- **C-28**: Any replay fingerprint mismatch stops execution and persists an inspectable recoverable divergence.
- **C-29**: Concurrent journal writers cannot lose, tear, or overwrite reservations/completions.
- **C-30**: New integrity records use correctly labeled SHA-256 digests.

### Results and artifacts

- **C-31**: One typed result record is authoritative for foreground return, background notification, and inspection.
- **C-32**: The result digest authenticates the complete semantic result record.
- **C-33**: Call artifacts are real, bounded, resolvable, and digest-verifiable.
- **C-34**: Child artifacts are real, bounded, resolvable, and digest-verifiable.
- **C-35**: Large final output uses a real artifact handle and bounded projection.
- **C-36**: Corrupt or missing result/artifact data fails closed with bounded diagnostics.

### Production truthfulness and evidence

- **C-37**: Normal production Tool Program construction exposes and accepts only `native_only`.
- **C-38**: No silent hosted-to-native fallback occurs.
- **C-39**: Process-level tests use a real daemon process, public submission boundary, migrated SQLite database, process kill, and restart.
- **C-40**: Notification concurrency uses independent service instances and connections.
- **C-41**: Closure-bearing tests assert observable production behavior rather than type, comment, string, or field existence.
- **C-42**: Full targeted formatting, compilation, migrations, and test suites pass.
- **C-43**: No unresolved high or medium authorization, delivery, descendant, recovery, integrity, resource, or evidence finding remains.
- **C-44**: Registry, addendum, implementation plan, architecture docs, commit SHAs, test evidence, and M013 closure record agree.
- **C-45**: An independent reviewer accepts `plans/closure/tool-programs/013-status.md` after inspecting production code and evidence.

## 18. Explicit rejection examples

The following do not satisfy this plan:

- constructing a grant from workspace/program/timestamp strings and calling it a permission decision;
- checking only `BrokerAuthority::Verified` without validating the grant contents;
- adding lineage fields to Rust structs without schema migration and store round trip;
- cancelling descendants only inside a parent future that the scheduler may drop;
- testing two `Arc` clones of the same in-memory notification service;
- calling reread of the same object a restart test;
- asserting two `Duration` values to prove capacity-one scheduler behavior;
- asserting a field is `Some` to prove recursive descendant convergence;
- checking that test files exist to prove process recovery;
- loading only completed calls and claiming checkpoint recovery;
- comparing only tool name and input and claiming full replay fingerprint binding;
- returning empty child artifact vectors while claiming artifact closure;
- hashing only `ProgramResult` while claiming full-record integrity;
- labeling MD5 output as SHA-256;
- recording a test command in a closure file without verifiable execution evidence;
- creating the closure record in the same implementation pass and self-accepting it.

## 19. Final handoff condition

The implementation model's final response must provide:

- ordered commit SHAs matching the required work-package sequence;
- files and migrations changed per work package;
- exact tests and commands executed;
- pass/fail counts;
- any skipped test with justification;
- remaining findings, if any;
- confirmation that the plan and registry were moved only to `closing`;
- confirmation that no M013 closure record was created or accepted.

If any C-01 through C-44 criterion is not satisfied, leave M013 `active` or `closing` and state the blocker precisely.
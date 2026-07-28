# Tool Programs Milestone 015 — Final Production-Path and Independent Closure

Status: closed

Class: final corrective implementation / authorization convergence / restart correctness / canonical persistence / process evidence / independent closure

Baseline reviewed:

- `c9559d23634771dc1bae742da43ae8e362507f6f`

Predecessor records:

- `plans/implementation/tool-programs/014-production-boundary-and-process-evidence-closure.md`
- `plans/closure/tool-programs/014-status.md`
- `plans/subsystems/tool-programs-correctness-closure-addendum.md`

Target closure record:

- `plans/closure/tool-programs/015-status.md`

## 1. Objective

Retain the substantial M014 implementation while correcting the remaining production-path mismatches and replacing structural or nominal evidence with mechanism-faithful proof.

M015 is the final strict-closure pass for the native-only Tool Programs line of work. It is complete only when the normal direct invocation path, scheduler, Broker, interpreter, SQLite stores, RunStore/artifact store, session delivery path, managed process layer, and real daemon process agree on one durable execution identity and converge correctly across failure and restart.

The implementation must establish all of the following:

1. a Tool Program job can be created only from an actual accepted permission and workspace path-policy decision;
2. correlation values and identity-derived hashes cannot substitute for authorization evidence;
3. submission, executor admission, and every nested Broker call verify the same frozen contract snapshot with the same canonical digest algorithm;
4. a normal authorized read-only nested call succeeds through the real production path;
5. checkpoint recovery cannot erase a newer durable completed call;
6. an active child wait is durably recorded before waiting and is reattached or reconciled after restart without duplicate submission;
7. call, child, and large-output artifacts use canonical resolvable stores and verified content digests;
8. notification creation, persistence, injection, and acknowledgement fail closed and remain durably idempotent across independent service instances and daemon restart;
9. descendant traversal crosses terminal intermediate nodes, while cancellation and restart reconciliation converge jobs, attempts, process groups, permits, leases, and counters;
10. real process tests submit through a public daemon protocol, activate deterministic failpoints, terminate the process, restart against the same durable state, and prove bounded convergence;
11. implementation and closure review remain separate and all evidence identifies the exact reviewed commit.

## 2. Handoff profile

This plan is intended for a smaller implementation model. Execute it in order and do not infer closure from field presence, comments, or manually assembled fixtures.

### 2.1 Required execution style

- Add a failing production-path test before each corrective behavior change.
- Keep the work packages in separate commits using the sequence in section 4. Do not squash before independent review.
- Reuse the existing permission decision, Broker catalog, scheduler, JobStore, RunStore, artifact store, session insertion, and daemon protocol abstractions.
- Do not add parallel authorization, storage, artifact, notification, or daemon systems.
- Security-relevant missing data must reject submission or execution. Do not synthesize a permissive fallback.
- Closure-bearing persistence failures must propagate as typed failures. Logging and continuing is not acceptable.
- Use the repository's intentional low-concurrency test configuration.
- After implementation and required tests pass, change this plan only from `ready` to `closing`.
- Do not create, approve, or mark `plans/closure/tool-programs/015-status.md` closed. An independent reviewer owns that file.

### 2.2 Non-goals

Do not:

- redesign the restricted Python language, parser, compiler, or instruction set;
- broaden the programmable palette beyond the existing read-only tools and scheduler-owned child operations;
- add shell, patch, Git mutation, commit, push, destructive, approval-sensitive, or subagent tools;
- implement hosted Tool Programs;
- replace the scheduler, JobStore, session store, RunStore, artifact store, or managed process layer wholesale;
- preserve a compatibility path that creates an executable grant when the accepted decision is absent;
- call a second object instance in one test process a daemon restart;
- mark a process test successful when the daemon binary cannot be spawned;
- use fabricated `ctx://` strings or arbitrary digest-shaped strings as artifact evidence;
- weaken or defer a binary closure criterion.

### 2.3 Preserve from M014

Retain and build on these valid improvements unless a failing test proves a defect:

- persisted `authority_grant_json` in the Tool Program job payload;
- integrity and validity verification in executor admission;
- expanded `InterpreterCheckpoint` and `ReplayFingerprint` types;
- checkpoint loading in the executor;
- v35 lineage migration and lineage preservation across common transitions;
- SHA-256 result-record integrity verification;
- native-only backend enforcement;
- file-locking work in the replay ledger;
- recursive descendant traversal structure, after correcting its terminal-node traversal semantics.

## 3. Verified remaining findings at the M014 baseline

### F01 — Authority still permits synthetic fallback material

`to_core_context()` and `build_authority_grant()` can substitute program, workspace, session, or agent-derived values when the accepted decision fields are missing. These values are useful for correlation but are not proof that permission was granted.

The current authority tests call the grant constructor directly with a workspace fixture. They do not prove that the normal accepted direct-call decision creates the grant or that a denied or missing decision creates no scheduler job.

### F02 — Contract verification uses incompatible digest algorithms

Submission computes a sorted catalog snapshot digest from multiple complete `ContractEntry` values. Broker scope verification computes a different per-tool digest over a smaller legacy field set and compares it directly with the catalog digest.

Submission also resolves contracts from a newly created default registry and converts resolution errors into an empty snapshot. This does not freeze the actual runtime Broker catalog and can permit a job with no valid snapshot.

### F03 — Checkpoint restoration can discard newer completed calls

The executor first loads the durable completed-call ledger and then restores a checkpoint. `restore_checkpoint()` replaces the completed-call map with the checkpoint copy. A crash after call completion but before checkpoint commit can therefore cause restart to forget the newer completion and repeat the call.

### F04 — Pending child wait identity is not persisted by the production interpreter

The checkpoint type contains `pending_child_wait`, but checkpoint creation always stores `None`. Child execution waits for completion before committing the next checkpoint. A daemon failure while a child is active therefore lacks durable reattachment identity.

### F05 — Child and output artifacts remain synthetic or noncanonical

Child tracking hashes `job_id:status`, leaves `run_id` absent, returns no child artifacts, and later uses the synthetic digest as an artifact ID. Large output is written directly under `.codegg`, a `ctx://` handle is constructed manually, and write failure only logs a warning.

The current artifact tests manually construct handles and digest-shaped strings instead of producing and resolving them through the production stores.

### F06 — Notification persistence still logs and continues

Notification creation writes the in-memory cache first, logs SQLite persistence failure, and returns apparent success. Recovery can convert a database error into zero recovered records. The executor cannot distinguish successful durable notification creation from a warning-only failure.

### F07 — Recursive traversal stops at terminal intermediate nodes

`find_descendants()` filters out terminal children before adding them to the traversal queue. An active grandchild beneath a terminal intermediate job is therefore not discovered.

### F08 — The M014 daemon suite is nominal rather than mechanism-faithful

The suite verifies binary presence, starts and kills a daemon without protocol submission or failpoint activation, skips successfully when spawning fails, and uses multiple ledger objects inside one process as restart evidence. It does not prove child reattachment, call replay, notification append-before-ack, process-group cleanup, or resource convergence.

### F09 — M014 closure was self-created and overstates evidence

The implementation commit also created the M014 closure record and claimed all criteria complete. The record is internally inconsistent: it says `closing`, names an independent reviewer, identifies the implementation as “this commit,” and concludes that M014 is closed.

M014 remains a valuable historical conditional implementation record. M015 owns final strict closure.

## 4. Required commit sequence

Use this sequence. Do not combine implementation and closure governance.

1. `test(tool-programs): add failing M015 accepted-decision and contract convergence coverage`
2. `fix(tool-programs): require accepted decision authority at submission`
3. `fix(tool-programs): freeze and verify one runtime contract snapshot`
4. `test(tool-programs): add failing M015 monotonic replay and child reattachment coverage`
5. `fix(tool-programs): merge checkpoint and call recovery monotonically`
6. `fix(tool-programs): persist and reattach active child waits`
7. `test(tool-programs): add failing M015 canonical artifact production coverage`
8. `fix(tool-programs): use canonical call child and output artifacts`
9. `test(tool-programs): add failing M015 notification fault and restart coverage`
10. `fix(tool-programs): make terminal notification delivery fail closed`
11. `test(scheduler): add terminal-intermediate descendant and resource convergence coverage`
12. `fix(scheduler): reconcile complete descendant graphs and resources`
13. `test(tool-programs): add real M015 daemon failpoint recovery harness`
14. `docs(plans): move Tool Programs M015 to closing`

A schema migration may share the commit with its compile-propagation changes. Explain any deviation from the sequence in the implementation handoff.

## 5. Work package A — Accepted-decision authority and contract convergence

### A1. Require an actual accepted decision

Locate the production permission and path-policy result used immediately before the direct `tool_program` invocation is allowed. Carry the minimum immutable non-secret decision snapshot into `ToolExecutionContext` and then into `ToolProgramExecutionContext`.

The executable grant constructor must require:

- a non-empty durable decision ID or reference;
- an explicit accepted/allowed outcome;
- principal identity;
- workspace identity and canonical path-policy ID;
- path-policy and permission-policy revisions;
- caller and maximum effect class;
- issued-at and any expiry/revocation state;
- session, turn, and agent identity when the decision is scoped to them.

Return a typed error before source persistence or scheduler submission when the decision is absent, denied, malformed, stale, expired, revoked, or does not match the current workspace and path policy.

Remove executable fallback behavior from `to_core_context()` and `build_authority_grant()`. Program ID, invocation key, source digest, and context hashes may remain as correlation fields only.

### A2. Freeze the actual runtime Broker contract snapshot

Use the same production `ToolRegistry` and `ToolBroker` catalog that will execute nested calls. Do not instantiate a separate default registry in the submission path.

Resolve every requested tool before creating a job. Reject:

- unknown tools;
- direct-only or otherwise non-programmable tools;
- mutation-capable or approval-sensitive effects;
- incomplete schemas or unstable implementation identity;
- duplicate or noncanonical tool names.

Any resolution error must reject submission. Remove `unwrap_or_default()` or equivalent empty-snapshot fallback.

Persist the bounded canonical snapshot or a durable immutable reference plus digest with the job/grant.

### A3. Use one digest helper everywhere

One canonical helper must serialize and hash the full sorted snapshot. Use that exact helper at:

1. direct submission;
2. executor admission;
3. every nested Broker invocation.

Broker verification must verify:

- requested tool membership;
- the exact persisted entry for that tool;
- the full snapshot digest;
- the current runtime contract identity against the persisted entry;
- grant, workspace, path-policy, principal, session, caller, effect, and policy-revision scope.

Do not compare a per-tool legacy digest with the full catalog digest.

### A4. Required tests

Create or replace the M014 authority tests with `tests/tool_program_m015_authority_contract.rs` using the normal production submission boundary.

Prove:

- accepted real decision → one persisted Tool Program job with the same decision identity;
- denied decision → no source persistence and no job;
- missing decision → no job;
- identity strings and hashes alone → no executable grant;
- stale/expired/revoked/mismatched decision → no job or pre-invocation rejection;
- runtime contract resolution failure → no job;
- normal authorized `read` or equivalent read-only nested call succeeds;
- reordered tool requests produce the same canonical snapshot digest;
- contract version, schema, effect, caller-policy, or catalog drift fails before invocation;
- executor contains no grant-construction path;
- default-registry substitution cannot bypass the injected runtime catalog.

Direct calls to `verify_integrity()` are supplementary only.

## 6. Work package B — Monotonic replay and active-child recovery

### B1. Define authoritative restart merge semantics

Treat checkpoint state and completed-call records as two durable streams with monotonic sequence identities.

On restart:

1. load the latest valid checkpoint;
2. load all valid completed calls;
3. reject conflicting records for the same sequence;
4. retain completed calls newer than the checkpoint;
5. set `next_call_seq` from the merged authoritative set and checkpoint state;
6. verify every replay fingerprint before using a completion;
7. never re-execute a durably completed call.

`restore_checkpoint()` must not erase newer completions already loaded from the ledger.

Choose one clear API, for example:

- restore checkpoint state, then merge completed calls with conflict detection; or
- construct a typed `RecoveredInterpreterState` from both stores and restore once.

Do not rely on load order as implicit conflict resolution.

### B2. Persist active child wait before blocking

Before awaiting a child job:

- reserve the canonical instruction/call sequence;
- submit or deduplicate the child through the scheduler;
- persist a checkpoint containing the child job ID, parent program/job/attempt, canonical call ID, instruction sequence, relation kind, and expected operation/config digest;
- only then await the child.

After completion:

- persist the typed completed child call/result;
- persist the next checkpoint clearing the active wait;
- ensure crash windows between each step are recoverable and idempotent.

On restart, if a valid pending child exists:

- query the scheduler/JobStore by durable child identity;
- reattach to an active child;
- consume an already terminal child result;
- fail closed on missing, conflicting, or lineage-mismatched child state;
- never submit a duplicate child for the same canonical sequence.

### B3. Preserve the original deadline

The original absolute deadline remains authoritative across restart. Recovery may reduce remaining time but may not reset or extend it.

The deadline must be included in checkpoint/replay integrity and tested across a process restart.

### B4. Required tests

Create `tests/tool_program_m015_recovery.rs` and prove:

- crash after call completion but before checkpoint commit does not repeat the call;
- checkpoint completion conflict fails closed with a typed divergence;
- newer completed calls survive checkpoint restoration;
- next sequence is correct after a sparse but valid merge;
- pending child identity is present before the await begins;
- restart reattaches the same child job;
- terminal child result is consumed without resubmission;
- mismatched lineage/config digest fails closed;
- original deadline is unchanged and never extended;
- overlapping ledger readers/writers do not corrupt state.

## 7. Work package C — Canonical results and artifacts

### C1. Call artifacts

For each completed nested tool call, retain the real artifact handles returned by the Broker and their content digests. Handles must resolve through the canonical artifact store.

If a tool returns no artifact, represent that as a typed absence rather than a fabricated digest.

### C2. Child artifacts and RunStore identity

Construct child result references from the actual scheduler attempt/run and RunStore/artifact records. Include:

- child job ID;
- child attempt ID and run ID when available;
- terminal status;
- canonical result or artifact handle;
- verified content digest;
- typed absence reason when the child operation legitimately produces no artifact.

Do not hash `job_id:status` and call it a result artifact.

### C3. Large output

Use the canonical artifact store API to persist oversized final output. The returned handle and digest must come from that store.

Artifact persistence failure must fail typed result commit and prevent the job from being reported as successfully completed. Do not write directly to a constructed `.codegg` path and do not fabricate `ctx://` handles.

### C4. One authoritative typed result

Foreground response, background notification, inspection, and restart recovery must load the same integrity-checked `ProgramResultRecord` and expose the same artifact identities.

The result digest must cover every semantic field and all artifact references.

### C5. Required tests

Create `tests/tool_program_m015_artifact_pipeline.rs` using real Broker/scheduler/store paths.

Prove:

- a real nested read call artifact can be resolved and its digest verified;
- a real child run produces the expected run/result identity or typed absence;
- oversized output is persisted through the canonical store and resolved by handle;
- injected artifact-store failure prevents successful terminal result commit;
- tampered result or artifact content fails closed;
- foreground, background, and inspection expose identical handles and digests;
- no test passes by manually assembling arbitrary handle or digest strings.

## 8. Work package D — Fail-closed notification delivery

### D1. Make SQLite authoritative

Notification creation must persist durably before returning success or exposing an actionable in-memory record.

Change closure-bearing APIs to return typed `Result` values. At minimum:

- `record_notification`;
- `record_terminal_result`;
- pool recovery/reconciliation;
- injection marking;
- acknowledgement.

A persistence or serialization failure must propagate to the executor or reconciliation owner. The program result may remain terminal, but notification delivery must be inspectably failed and retryable; it must not be silently reported as delivered or recoverable from memory alone.

### D2. Durable append-before-ack identity

Use one schema-enforced injection key and session event identity. The intended sequence is:

1. claim durable notification;
2. append/deduplicate the parent session event using the durable injection key;
3. persist the injected event ID;
4. acknowledge delivery.

After a crash at any boundary, an independent service instance must either complete acknowledgement of the existing event or perform one deduplicated append. It must never append twice.

### D3. Independent service and restart tests

Create `tests/tool_program_m015_notification_recovery.rs` using two independent SQLite pools/service instances and the real session insertion boundary.

Prove:

- SQLite write failure returns an error and no actionable in-memory success;
- claim contention has one winner;
- crash after claim but before append is retryable;
- crash after append but before mark/ack does not duplicate the event;
- delivered records are not recreated by terminal-job reconciliation;
- database query failure is distinguishable from zero recovered records;
- payload and event identities are correctly labeled SHA-256 where applicable;
- parent session receives exactly one actionable terminal event.

## 9. Work package E — Complete descendant graph and resource convergence

### E1. Traverse through terminal intermediates

Descendant discovery must traverse every lineage edge regardless of the intermediate job's terminal state. Return or act on nonterminal descendants, but do not prune the graph before visiting their children.

Use a visited set and deterministic ordering. Detect malformed cycles and return bounded diagnostics.

### E2. Scheduler-owned reconciliation

The scheduler/recovery owner must reconcile descendants after parent:

- cancellation;
- timeout;
- interruption;
- lost worker;
- daemon-generation change;
- restart recovery.

Prove bounded convergence of:

- child and grandchild job state;
- active attempts;
- managed process groups;
- scheduler permits;
- workspace/build leases;
- active counters and queue capacity.

Parent executor cleanup may remain as defense in depth but is not the source of truth.

### E3. Required tests

Create `tests/tool_program_m015_descendant_convergence.rs` and prove:

- active grandchild beneath a terminal child is discovered and cancelled;
- deeper mixed terminal/nonterminal lineage converges;
- malformed cycle does not loop indefinitely;
- timeout/cancel/restart paths produce the same terminal convergence;
- managed process group is gone;
- permit, lease, and active counters return to baseline;
- a capacity-one scheduler can run the next unrelated job after convergence.

## 10. Work package F — Real daemon failpoint recovery harness

### F1. Test-only failpoints in production mechanisms

Add narrowly scoped, test-gated failpoints at the production boundaries needed to reproduce crash windows. Suggested points:

- after accepted grant/job persistence but before executor start;
- after call completion persistence but before checkpoint commit;
- after child submission and pending-wait checkpoint but before await completion;
- after result commit but before terminal notification persistence;
- after session append but before notification injected-ID persistence/ack;
- after descendant cancellation request but before process/resource reconciliation completes.

Failpoints must be disabled in normal builds and must not introduce alternate production logic.

### F2. Public protocol process harness

Create `tests/tool_program_m015_daemon_failpoints.rs` or an equivalent bounded integration harness.

Each scenario must:

1. build or locate the real daemon binary; inability to spawn is a test failure on the primary supported CI platform;
2. create one temporary daemon home, SQLite catalog, workspace, and protocol endpoint;
3. start daemon process A and wait for an explicit readiness signal;
4. submit a Tool Program through a public supported protocol boundary;
5. activate a deterministic failpoint and wait until it is reached;
6. terminate daemon process A;
7. start fresh daemon process B against the same durable state;
8. inspect results through public protocol and durable stores;
9. assert exact-once execution/delivery and bounded resource convergence;
10. terminate daemon B and verify no managed child process remains.

Do not share in-memory services, scheduler objects, ledgers, registries, or caches between process A and process B.

### F3. Mandatory scenarios

The real process harness must cover:

- accepted authority and normal nested read success;
- call completion persisted before checkpoint;
- active child reattachment;
- result committed before notification persistence;
- session append before acknowledgement;
- recursive descendant/process cleanup;
- corrupt checkpoint or result rejection;
- original deadline preserved across restart.

Tests may use bounded polling with explicit timeouts. Arbitrary sleeps without readiness/state assertions are insufficient.

## 11. Work package G — Final evidence and governance

### G1. Implementation handoff

The implementation model must:

- leave this plan at `closing`, not `closed`;
- record exact implementation commit identities;
- list every required command and its result;
- disclose skipped platform-specific tests and why;
- leave `plans/closure/tool-programs/015-status.md` absent.

### G2. Independent closure review

A separate reviewer must inspect production mechanisms, rerun the required test set, verify CI/status evidence, and create `plans/closure/tool-programs/015-status.md` in a later commit.

The closure record must contain:

- exact implementation head reviewed;
- exact independent review commit;
- criterion-by-criterion disposition;
- test commands and observed outcomes;
- CI workflow/status references when available;
- confirmation that no high or medium finding remains;
- confirmation that plan, addendum, architecture documentation, and registry agree.

The reviewer must not accept manually constructed fixtures, comments, type presence, or nominal process startup as proof of production behavior.

## 12. Required validation commands

Run at minimum:

```bash
cargo fmt --all -- --check
cargo check -p codegg --all-targets
cargo test -p codegg --test tool_program_m015_authority_contract -- --test-threads=1
cargo test -p codegg --test tool_program_m015_recovery -- --test-threads=1
cargo test -p codegg --test tool_program_m015_artifact_pipeline -- --test-threads=1
cargo test -p codegg --test tool_program_m015_notification_recovery -- --test-threads=1
cargo test -p codegg --test tool_program_m015_descendant_convergence -- --test-threads=1
cargo test -p codegg --test tool_program_m015_daemon_failpoints -- --test-threads=1
cargo test -p codegg tool_program -- --test-threads=1
cargo test -p codegg-core tool_program -- --test-threads=1
scripts/check-core-boundary.sh
python3 scripts/check_scheduler_bypass.py
python3 scripts/check_execution_ownership.py
```

Also run migration tests from a pre-v35 database fixture through the latest schema if the implementation changes storage again.

Do not claim repository-wide success unless the corresponding command was actually run and completed.

## 13. Binary closure criteria

### Authority and contracts

- **C-01** An actual accepted direct-call decision is required before source persistence or job creation.
- **C-02** Denied, missing, malformed, stale, expired, revoked, or workspace-mismatched decisions create no executable job.
- **C-03** Program/workspace/session/agent strings and hashes cannot create executable authority.
- **C-04** The persisted grant retains the immutable accepted decision identity across job-store round trip and daemon restart.
- **C-05** The executor never constructs, repairs, or widens a grant.
- **C-06** Submission freezes the actual injected runtime Broker contract catalog.
- **C-07** Contract resolution error rejects submission; no empty-snapshot fallback exists.
- **C-08** Submission, executor, and Broker use one canonical snapshot digest helper.
- **C-09** Normal authorized nested read-only execution succeeds through the production path.
- **C-10** Membership, version, schema, caller-policy, effect, or catalog drift fails before invocation.

### Replay, checkpoint, and child recovery

- **C-11** Recovery merges checkpoint and completed-call records monotonically.
- **C-12** A newer durable call completion cannot be erased by an older checkpoint.
- **C-13** Conflicting completion records fail with typed replay divergence.
- **C-14** `next_call_seq` is correct after recovery.
- **C-15** Active child identity is durably checkpointed before waiting.
- **C-16** Restart reattaches the same active child or consumes its terminal result.
- **C-17** Restart never submits a duplicate child for the same canonical sequence.
- **C-18** Child lineage/config mismatch fails closed.
- **C-19** Original absolute deadline is retained and never extended.
- **C-20** Overlapping process access does not corrupt replay/checkpoint state.

### Results and artifacts

- **C-21** Real call artifact handles resolve through the canonical store and verify by digest.
- **C-22** Child result references contain actual job/attempt/run identity or a typed absence reason.
- **C-23** Synthetic `job_id:status` digests are not used as child artifacts.
- **C-24** Large output is persisted through the canonical artifact store.
- **C-25** Artifact persistence failure prevents successful result commit.
- **C-26** Foreground, background, inspection, and restart expose one authoritative result and identical artifact identities.
- **C-27** Result integrity covers every semantic and artifact field.
- **C-28** Missing, corrupt, or tampered result/artifact data fails closed.

### Notification delivery

- **C-29** Durable notification persistence occurs before actionable success is returned.
- **C-30** Notification persistence/query/serialization failures propagate as typed errors.
- **C-31** Independent service instances have one durable claim winner.
- **C-32** Session insertion uses a schema-enforced durable injection key.
- **C-33** Crash after append and before ack does not duplicate the parent event.
- **C-34** Delivered records are not recreated by terminal reconciliation.
- **C-35** Recovery distinguishes storage failure from zero records.
- **C-36** Parent session receives exactly one actionable terminal notification.

### Descendants and resources

- **C-37** Traversal crosses terminal intermediate nodes.
- **C-38** Active descendants at arbitrary supported depth are discovered and reconciled.
- **C-39** Cycles are bounded and diagnosed.
- **C-40** Cancel, timeout, interruption, lost-worker, generation-change, and restart paths converge equivalently.
- **C-41** Descendant process groups are gone after convergence.
- **C-42** Permits, leases, counters, and capacity return to baseline.

### Real process evidence and governance

- **C-43** A real daemon accepts Tool Program submission through a public protocol.
- **C-44** Deterministic failpoints stop the real production path at required crash windows.
- **C-45** A fresh daemon process resumes against the same durable state without shared in-memory objects.
- **C-46** Process tests prove exact-once call execution, child submission, and parent notification.
- **C-47** Process tests prove deadline retention, corruption rejection, descendant cleanup, and resource convergence.
- **C-48** Spawn/readiness/protocol failures fail tests rather than skip successfully on the primary CI platform.
- **C-49** Required targeted tests, formatting, compilation, migrations, and static guards pass at the exact implementation head.
- **C-50** The implementation pass leaves M015 at `closing` and does not create or approve its closure record.
- **C-51** An independent reviewer creates the closure record in a later commit with exact evidence.
- **C-52** No unresolved high or medium finding remains and all governance documents agree.

All C-01 through C-52 are mandatory.

## 14. Explicit rejection examples

Reject the implementation as incomplete if any of the following remains:

- a missing accepted decision produces `program:{id}`, `workspace:{id}`, or another executable fallback;
- submission creates a default registry instead of using the runtime Broker catalog;
- contract resolution uses `unwrap_or_default()` or continues with an empty snapshot;
- Broker compares a per-tool legacy digest with the full catalog digest;
- restoring a checkpoint replaces and loses a newer completed-call record;
- `pending_child_wait` is always `None` in production checkpoints;
- child artifact identity is a hash of job ID and status;
- large output is written directly to a constructed path or receives a fabricated `ctx://` handle;
- notification persistence failure only logs a warning;
- database recovery failure is returned as zero recovered records;
- descendant traversal prunes terminal intermediate nodes;
- daemon tests return successfully when the binary cannot be spawned;
- daemon tests do not submit through a public protocol or activate failpoints;
- process A and process B share in-memory objects;
- the implementation commit creates or accepts `015-status.md`;
- documentation claims strict closure without an independent reviewed commit.

## 15. Final completion definition

M015 may move to `closing` after production implementation and all required local tests pass.

The Tool Programs subsystem may move to `closed` only after a separate reviewer confirms all C-01 through C-52 at the exact reviewed head and creates `plans/closure/tool-programs/015-status.md` in a later commit.

After independent acceptance:

- M011 through M014 remain historical conditional implementation records;
- M015 becomes the final strict closure record for native-only Tool Programs;
- deferred hosted execution and programmable-palette expansion remain separate, unregistered product work;
- no additional corrective milestone should be created unless a new production-path defect is demonstrated.

## 16. Implementation handoff

Implementation head: `247ef50`

Implementation commits, in order:

- `e22ceb06`, `bc3e8b32`, `27bbb834`
- `2d5ab5a3`, `85d2f9a7`, `8415d81b`
- `af8a3c5b`, `351edb85`
- `3bfa10e1`, `de432a8c`
- `280365de`, `143a8b59`
- `ffcac3d3`, `b10716a0`
- `6dc63ab6`, `73b8db6b`, `aec7284c`
- `247ef50`

Observed validation at the implementation head:

- `cargo fmt --all -- --check` — passed.
- `cargo check -p codegg --all-targets` — passed; existing warnings remain non-fatal.
- `cargo test -p codegg --test tool_program_m015_authority_contract -- --test-threads=1` — 5 passed.
- `cargo test -p codegg --test tool_program_m015_recovery -- --test-threads=1` — 5 passed.
- `cargo test -p codegg --test tool_program_m015_artifact_pipeline -- --test-threads=1` — 4 passed.
- `cargo test -p codegg --test tool_program_m015_notification_recovery -- --test-threads=1` — 9 passed.
- `cargo test -p codegg --test tool_program_m015_descendant_convergence -- --test-threads=1` — 8 passed.
- `cargo test -p codegg --test tool_program_m015_daemon_failpoints -- --test-threads=1` — 8 passed.
- `cargo test -p codegg --lib tool_program -- --test-threads=1` — 39 passed.
- `cargo test -p codegg-core tool_program -- --test-threads=1` — 156 passed.
- `cargo test -p codegg-core event_store_idempotency_tests` — 2 passed.
- `scripts/check-core-boundary.sh` — passed.
- `python3 scripts/check_scheduler_bypass.py` — passed.
- `python3 scripts/check_execution_ownership.py` — passed.
- `python3 scripts/e2e/tool_program_harness.py --mode native --scenario all` — passed through the M015 real-daemon matrix.

No platform-specific test was skipped. The process suite ran on macOS arm64 using the real debug `codegg core-stdio` binary. No storage schema version was added in M015, so a new pre-v35 migration fixture was not required; the existing v35 lineage columns were corrected at their SQLite insert/read production sites.

`cargo clippy --workspace --all-features --all-targets -- -D warnings` remains
blocked by three pre-existing `clippy::question_mark` findings in
`crates/egglsp/src/edit.rs`; M015 does not touch that crate or those findings.

Per the separation rule, this implementation handoff does not create or approve `plans/closure/tool-programs/015-status.md`. A later review commit owns independent acceptance.

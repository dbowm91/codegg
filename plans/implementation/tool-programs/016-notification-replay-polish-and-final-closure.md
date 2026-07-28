# Tool Programs Milestone 016 — Notification Replay Polish and Final Closure

Status: ready for handoff

Class: narrow restart-correctness polish / durable session injection / independent closure

Reviewed baseline:

- `9bd9d0bf1e27a021e5610fb8564ca601fda775c0`

Historical implementation head retained:

- M015 implementation `247ef5015d79bdd834bffca15c76ebb2426beb40`

Target independent closure record:

- `plans/closure/tool-programs/016-status.md`

Canonical direction:

- `plans/subsystems/tool-programs-roadmap.md`
- `plans/subsystems/tool-programs-correctness-closure-addendum.md`
- `plans/adrs/ADR-0001-programmatic-tool-execution-authority.md`

## 1. Objective

Close the single demonstrated post-M015 production defect in background Tool Program notification recovery without reopening the broader Tool Programs architecture.

M015 correctly changed parent-session delivery to append a durable session event before marking the notification injected and before acknowledgement. However, the retry path reconstructs that event with a new `created_at` value. `EventStore::append_idempotent` currently compares the complete serialized payload after an event-ID conflict. A process crash after the session append but before `mark_injected` can therefore make process B reuse the same stable event ID with different serialized metadata, receive an identity-collision error, and leave the notification permanently claimed or pending.

M016 must make reconstruction of the same logical notification event idempotent across process restart while continuing to reject any reuse of the event identity for different session, notification, program, injection key, or content.

This milestone is complete only when the real append-before-mark failpoint:

1. appends exactly one durable parent-session event in process A;
2. terminates process A before notification state is marked injected;
3. reconstructs the logical event in process B;
4. accepts the reconstruction as the same semantic event despite non-authoritative timestamp differences;
5. persists the injection marker and acknowledgement;
6. produces no duplicate parent event and no permanently retrying notification.

## 2. Scope boundaries

### In scope

- `ToolProgramNotificationEvent` durable identity and semantic equality;
- `EventStore` idempotent append behavior for the notification event type;
- deterministic event-envelope construction where practical;
- recovery of already appended but not marked-injected notifications;
- the `tool_program_after_session_append` two-process failpoint fixture;
- notification-service contention and restart regression coverage;
- M015/M016 planning and closure reconciliation.

### Out of scope

- changing Tool Program authority, contract snapshots, interpreter replay, child reattachment, artifacts, descendant cancellation, or native-only policy;
- broad changes to general session-event identity semantics unless required for a typed reusable helper;
- adding hosted Tool Programs or expanding the programmable palette;
- changing user-visible notification text or creating a second delivery channel;
- schema migration solely to store redundant data when the existing durable notification record can supply stable semantic identity.

## 3. Mandatory implementation rules

1. Add a failing unit regression and a failing real process-restart regression before the production fix.
2. Preserve the stable event ID `tp-event:{injection_key}`.
3. Treat `created_at` as metadata, not as part of the logical idempotency identity for this event type.
4. Compare every authority-bearing and content-bearing field when accepting an existing event:
   - event type;
   - session ID;
   - injection key;
   - notification ID;
   - program ID;
   - exact notification content or a canonical SHA-256 digest of it.
5. Reuse of the same event ID with different semantic content must fail closed.
6. An existing matching event must be accepted regardless of reconstructed timestamp differences; the stored event remains authoritative.
7. Do not delete or rewrite an already appended event to make a retry pass.
8. Do not mark a notification injected unless the durable event append or semantic-match check succeeds.
9. Do not acknowledge a notification unless its durable event identity has been recorded as injected.
10. Storage/query/serialization errors remain typed failures and must not be converted to success or zero work.
11. The implementation pass moves M016 only to `closing` and must not create `plans/closure/tool-programs/016-status.md`.
12. A separate reviewer must inspect the production mechanism and rerun the required tests before strict closure.

## 4. Default design decision

Use typed semantic idempotency for `ToolProgramNotificationEvent`.

The preferred implementation is:

- add a notification-specific semantic identity or comparison helper on `ToolProgramNotificationEvent` or `SessionEvent`;
- on `session_events.id` conflict, load the stored event;
- require the stored event to deserialize as `SessionEvent::ToolProgramNotification`;
- compare the immutable semantic fields listed above;
- ignore only non-authoritative reconstruction metadata such as `created_at`;
- return success for an exact semantic match and an explicit collision error otherwise.

The delivery path should also construct `created_at` deterministically from durable notification data when a suitable immutable timestamp already exists. That reduces incidental differences but does not replace semantic conflict handling, because already persisted M015 events may contain a timestamp chosen immediately before the crash.

A generic event-store helper is acceptable only if its comparison contract is explicit and cannot weaken identity checks for other event types. Do not change all session events to content-insensitive conflict acceptance.

## 5. Work packages

### Work package A — Reproduce the defect precisely

Add a unit regression that:

1. creates two `ToolProgramNotificationEvent` values with the same event ID, session, injection key, notification ID, program ID, and content;
2. gives them different `created_at` values;
3. appends the first;
4. appends the reconstructed second;
5. expects success and exactly one stored event.

Add negative cases proving that the same ID with any changed semantic field fails:

- different session ID;
- different injection key;
- different notification ID;
- different program ID;
- different content;
- different event variant.

The existing identical-instance test is retained but is not sufficient closure evidence.

### Work package B — Implement typed semantic append idempotency

Refactor `EventStore::append_idempotent` or introduce a narrowly typed notification append method so that:

- the first insert remains a normal durable insert;
- conflict handling loads the existing event;
- identical serialized events remain accepted;
- reconstructed Tool Program notification events use semantic equality;
- timestamp-only differences are accepted;
- semantic collisions return a typed storage/identity error;
- missing rows after a conflict remain an error;
- malformed stored payloads remain an error;
- no update or delete is performed during conflict reconciliation.

Use canonical SHA-256 only if content comparison is represented by a digest. Do not introduce MD5 or process-random hashing.

### Work package C — Complete delivery-state convergence

Audit the delivery order in `AgentLoop` and `ToolProgramNotificationService`:

```text
claim notification
    -> append or semantically confirm parent-session event
    -> mark injected with stable event ID
    -> acknowledge delivered
```

Required behavior:

- a crash after append but before mark is recoverable;
- a crash after mark but before acknowledge is recoverable;
- a matching pre-existing event allows mark and acknowledge to continue;
- a semantic collision prevents mark and acknowledgement;
- an event-store error prevents mark and acknowledgement;
- a notification-store error leaves durable state recoverable by another owner;
- repeated recovery reaches `delivered` without adding another parent event.

Do not rely on an in-memory `messages` insertion as the source of truth. The durable session event is authoritative.

### Work package D — Real process restart evidence

Extend `tests/tool_program_m015_daemon_failpoints.rs` or add `tests/tool_program_m016_notification_replay.rs` with a real two-process fixture using the existing debug process-owner failpoint capability.

The test must:

1. start process A against a temporary workspace and durable database;
2. create a real background Tool Program terminal notification;
3. activate `tool_program_after_session_append`;
4. wait for the failpoint marker proving the session append completed;
5. kill process A;
6. start process B against the same workspace/database without shared in-memory objects;
7. trigger notification recovery through the production boundary;
8. assert one and only one `tool_program_notification` session event exists;
9. assert the notification is marked injected with that event ID;
10. assert the notification reaches delivered/acknowledged state;
11. restart or recover again and prove the state remains stable;
12. fail rather than skip if either process cannot start or the expected marker/state is absent.

The fixture must not preconstruct the second event object and carry it across processes. Process B must reconstruct it from durable state, reproducing the real defect.

### Work package E — Independent closure and documentation

After production code and tests land:

- move this plan from `ready` to `closing`;
- update the registry and closure addendum to show M016 closing;
- leave `plans/closure/tool-programs/016-status.md` absent;
- retain M015 as a historical conditionally closed implementation record;
- record exact implementation commit and command evidence in the plan or a separate implementation evidence document.

A separate reviewer then:

- inspects the event identity and delivery order in production code;
- reruns all M016 tests and the affected M015 notification/daemon suites;
- confirms no semantic collision is silently accepted;
- confirms the append-before-mark process test reconstructs state in process B;
- creates `plans/closure/tool-programs/016-status.md` only if every criterion passes;
- updates roadmap, addendum, plan, closure record, and registry to `closed` in a later commit.

## 6. Required commit sequence

Use separate commits in this order:

1. `test(tool-programs): reproduce reconstructed notification event collision`
2. `test(tool-programs): add append-before-mark two-process recovery regression`
3. `fix(tool-programs): make notification session events semantically idempotent`
4. `fix(tool-programs): converge append mark and acknowledgement recovery`
5. `test(tool-programs): harden notification semantic collision coverage`
6. `docs(plans): move Tool Programs M016 to closing`

The implementation agent must not combine commit 6 with an M016 closure record.

## 7. Required verification

Run with bounded test concurrency:

```text
cargo fmt --all -- --check
cargo check -p codegg-core
cargo check -p codegg --all-features
cargo test -p codegg-core event_store_idempotency_tests -- --test-threads=1
cargo test -p codegg --test tool_program_m016_notification_replay -- --test-threads=1
cargo test -p codegg --test tool_program_m015_notification_recovery -- --test-threads=1
cargo test -p codegg --test tool_program_m015_daemon_failpoints -- --test-threads=1
cargo test -p codegg --test tool_program_notifications -- --test-threads=1
cargo test -p codegg --lib tool_program -- --test-threads=1
cargo test -p codegg-core tool_program -- --test-threads=1
python3 scripts/e2e/tool_program_harness.py --mode native --scenario all
bash scripts/check-core-boundary.sh
python3 scripts/check_scheduler_bypass.py
python3 scripts/check_execution_ownership.py
python3 scripts/check_daemon_cwd_usage.py
python3 scripts/check_tool_broker_boundary.py
```

If the repository-wide Clippy command still fails only on the previously recorded unrelated `crates/egglsp/src/edit.rs` findings, record that separately. Any new warning in changed M016 production or test files is a closure blocker.

## 8. Binary acceptance criteria

### Event identity

- C-01: `tp-event:{injection_key}` remains the stable durable event ID.
- C-02: a timestamp-only difference does not cause a collision for the same logical Tool Program notification event.
- C-03: the first append creates exactly one durable event.
- C-04: a matching reconstructed append returns success without updating or duplicating the stored event.
- C-05: session-ID mismatch fails closed.
- C-06: injection-key mismatch fails closed.
- C-07: notification-ID mismatch fails closed.
- C-08: program-ID mismatch fails closed.
- C-09: content mismatch fails closed.
- C-10: event-variant mismatch fails closed.
- C-11: malformed or missing stored conflict rows fail closed.
- C-12: event-store serialization or query failure is propagated.

### Delivery convergence

- C-13: notification marking occurs only after append or semantic confirmation succeeds.
- C-14: acknowledgement occurs only after the durable injected identity is recorded.
- C-15: an append error leaves the notification recoverable and unacknowledged.
- C-16: a semantic collision leaves the notification unmarked and unacknowledged.
- C-17: a mark-injected failure does not create a second event on retry.
- C-18: an acknowledgement failure retries without creating a second event.
- C-19: two independent service owners cannot create two parent-session events.
- C-20: repeated recovery converges to delivered state.

### Process evidence

- C-21: process A appends the event and reaches the failpoint before termination.
- C-22: process B uses the same durable database with no shared memory from process A.
- C-23: process B reconstructs the event rather than receiving a prebuilt event from the test process.
- C-24: process B accepts the existing semantic event despite timestamp reconstruction differences.
- C-25: exactly one parent-session event exists after recovery.
- C-26: the notification records the stable injected event ID after recovery.
- C-27: the notification reaches delivered/acknowledged state after recovery.
- C-28: an additional restart/recovery remains stable and duplicate-free.
- C-29: process startup, marker, and recovery assertion failures fail the test rather than skip or return success.

### Regression and governance

- C-30: affected M015 notification and daemon suites pass.
- C-31: broader Tool Program library/core tests pass with bounded concurrency.
- C-32: native harness and architecture/static guards pass.
- C-33: no authority, contract, child, artifact, descendant, or native-only invariant is weakened.
- C-34: the implementation pass leaves M016 at `closing` and does not create its closure record.
- C-35: a separate reviewer records the exact implementation head and independently reruns the M016 and affected M015 suites.
- C-36: roadmap, addendum, implementation plan, closure record, and registry agree before strict closure.
- C-37: no unresolved high or medium Tool Programs finding remains.

All C-01 through C-37 are mandatory.

## 9. Rejected shortcuts

The following do not satisfy M016:

- removing `created_at` from serialized session events globally;
- accepting every event-ID conflict without comparing semantic content;
- comparing only event ID and event type;
- rewriting the stored event with the retry payload;
- deleting the stored event and reinserting it;
- marking injected before confirming the durable session event;
- acknowledging after a logged append or mark error;
- using an in-memory event instance in both sides of the restart test;
- using two service objects in one process as the sole process evidence;
- disabling the failpoint test when process startup fails;
- self-creating or self-approving `016-status.md` in the implementation pass.

## 10. Final closure review checklist

The independent reviewer must answer all of the following with code and test evidence:

1. Which fields define the semantic identity of a Tool Program notification event?
2. Which fields are intentionally excluded, and why can they not change authorization or visible content?
3. Can the same event ID be reused for different content, session, notification, or program identity?
4. Does a timestamp-only reconstructed retry succeed without modifying the stored event?
5. Does process B reconstruct the event exclusively from durable state?
6. Does the append-before-mark fixture prove one event and eventual delivered state?
7. Do append, mark, and acknowledgement failures each remain recoverable and fail closed?
8. Did all required bounded tests, harness checks, and static guards pass at the exact reviewed implementation head?
9. Is the closure record authored in a commit later than the implementation head?
10. Are all planning documents synchronized without claiming hosted execution or palette expansion?

Strict native-only Tool Programs closure may be restored only after all ten answers are affirmative and C-01 through C-37 are accepted.
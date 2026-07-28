# Tool Programs Milestone 017 — Semantic Recovery Confirmation and Evidence Closure

Status: ready for handoff

Class: final corrective implementation / semantic recovery confirmation / typed persistence errors / durable process evidence / independent closure

Reviewed baseline:

- `f4101b9cb739889f65c92aa747e6869d49241c88`

Retained implementation foundations:

- M015 implementation `247ef5015d79bdd834bffca15c76ebb2426beb40`
- M016 implementation `f4101b9cb739889f65c92aa747e6869d49241c88`

Target independent closure record:

- `plans/closure/tool-programs/017-status.md`

Canonical direction:

- `plans/subsystems/tool-programs-roadmap.md`
- `plans/subsystems/tool-programs-correctness-closure-addendum.md`
- `plans/adrs/ADR-0001-programmatic-tool-execution-authority.md`

## 1. Objective

Close the remaining production-path defects in M016 notification recovery and produce mechanism-faithful evidence for strict native-only Tool Programs closure.

M016 correctly introduced semantic equality for reconstructed `ToolProgramNotificationEvent` values and made `EventStore::append_idempotent` accept timestamp-only reconstruction differences while rejecting semantic collisions. The recovery loop does not consistently use that semantic boundary. Its existing-event branch calls `has_event(event_id)` and, on any matching ID, immediately marks the notification injected and acknowledges it without loading or comparing the stored event. The caller also converts an event-store query failure to `false` with `unwrap_or(false)`.

The current process fixture proves that a nominal existing event can lead to a recovery report, but it does not directly prove the durable notification row, injected event identity, exact event count, semantic event contents, delivered timestamp, or stability after an additional process restart.

M017 must establish one recovery rule:

> A notification may move to injected or delivered only after the durable parent-session event has either been inserted through `append_idempotent` or loaded and semantically confirmed against the event reconstructed from the durable notification record.

Existence alone is not confirmation. Query failure is not absence. Job completion is not notification delivery evidence.

## 2. Scope boundaries

### In scope

- `ToolProgramNotificationEvent` reconstruction from durable notification records;
- typed loading and semantic confirmation of existing session events;
- `EventStore` APIs used by notification recovery;
- `inject_recoverable_notifications` state transitions and error propagation;
- Pending, Claimed, injected, and Delivered notification recovery cases;
- direct durable-state evidence for event count and notification delivery state;
- the append-before-mark real process fixture and one additional restart;
- M016 conditional disposition, roadmap synchronization, registry synchronization, and independent M017 closure.

### Out of scope

- authority, contract snapshot, interpreter, call replay, child-job, artifact, descendant, scheduler, or native-only redesign;
- hosted Tool Programs or programmable-palette expansion;
- changing ordinary session-event idempotency rules outside a shared typed helper;
- treating the debug recovery fixture request as a general user-facing API;
- changing notification prose unless required to make reconstruction deterministic;
- unrelated repository-wide Clippy cleanup in `crates/egglsp`.

## 3. Findings that M017 owns

### F01 — Existing event ID is treated as semantic confirmation

Current recovery performs an existence check and then calls `mark_injected` and `acknowledge`. It does not verify that the stored row is a `ToolProgramNotification` for the expected session, injection key, notification ID, program ID, and content.

Impact: a malformed, unrelated, or semantically conflicting event that reuses the stable ID can incorrectly authorize notification delivery state transitions.

### F02 — Event-store query failures are converted to absence

Current recovery uses `event_store.has_event(&event_id).await.unwrap_or(false)`.

Impact: database/query failure can be misclassified as “no event.” Pending work may continue down an insertion path and Claimed work may be reported as leased rather than as a typed storage failure. This violates fail-closed recovery and obscures operational faults.

### F03 — Process evidence does not directly prove durable notification state

The M016 process test checks a recovery report and the Tool Program job’s completed state. It does not directly assert:

- exactly one stored `tool_program_notification` session event;
- the event’s semantic fields;
- the notification row’s `state = delivered`;
- the stable `injected_event_id`;
- a non-null delivered timestamp;
- stable state after a third process starts and recovers again.

Impact: the test can pass while the notification persistence contract is incomplete.

### F04 — Planning state is inconsistent

The registry and closure addendum show M016 closing, while the canonical Tool Programs roadmap still reports strict closure at M015 and has no M016/M017 disposition.

Impact: the repository lacks one authoritative, internally consistent closure state.

## 4. Required design

### 4.1 Reconstruct one expected semantic event

Introduce one helper used by every notification recovery branch, for example:

```rust
fn expected_notification_event(
    notification: &ToolProgramNotification,
) -> Result<SessionEvent, NotificationRecoveryError>
```

The helper must derive:

- `meta.id = format!("tp-event:{}", injection_key)`;
- `meta.session_id = notification.session_id`;
- `injection_key` from the durable notification;
- `notification_id` from the durable notification;
- `program_id` from the durable notification;
- `content` from the same deterministic classification/summary formatter used for first delivery;
- `created_at` from durable notification data when practical, or a current timestamp if semantic equality explicitly excludes it.

No branch may reconstruct different content for the same durable notification.

### 4.2 Add typed existing-event confirmation

Add an `EventStore` API with explicit outcomes, for example:

```rust
pub enum ExistingEventConfirmation {
    Absent,
    SemanticMatch,
}

pub async fn confirm_existing(
    &self,
    expected: &SessionEvent,
) -> Result<ExistingEventConfirmation, StorageError>
```

Required semantics:

- query by the expected event ID;
- `Absent` only when the query succeeds and no row exists;
- deserialize and validate the stored event;
- require stored session ID and stored event type to match the expected event;
- for `ToolProgramNotification`, require `semantic_equals` to succeed;
- return `SemanticMatch` only after all semantic fields match;
- return a typed identity-collision or malformed-payload error for any mismatch;
- propagate query and deserialization failures;
- never update, delete, or replace the stored row.

A shared private comparison helper may serve both `append_idempotent` and `confirm_existing` so the two paths cannot drift.

Remove `has_event` from production recovery. It may be deleted entirely or retained only for narrow tests, but no state transition may depend on existence-only evidence.

### 4.3 Define recovery by notification state

The implementation must follow this state machine.

#### Already injected

```text
injected_event_id is present
    -> verify it equals the stable expected event ID
    -> confirm the durable event semantically
    -> acknowledge
```

A mismatched injected ID, missing event, semantic collision, or store error must prevent acknowledgement and produce a typed recovery error.

#### Claimed

```text
claimed notification
    -> reconstruct expected event
    -> confirm_existing(expected)
       -> SemanticMatch: mark injected, then acknowledge
       -> Absent: leave claimed for lease expiry; do not insert and do not acknowledge
       -> Error/collision: report error; do not mark or acknowledge
```

A Claimed notification may not create a new event because another live owner may still hold the lease.

#### Pending

```text
pending notification
    -> claim through notification-store CAS
    -> reconstruct expected event
    -> append_idempotent(expected)
    -> failpoint may terminate process here
    -> mark injected with stable event ID
    -> acknowledge delivered
```

An append collision or storage error leaves the notification unacknowledged and recoverable through lease expiry. Do not convert the error to a skipped success.

### 4.4 Direct durable-state inspection

Add a bounded read-only inspection mechanism for the test fixture. Preferred options, in order:

1. use production store APIs from the integration test after opening the same durable catalog;
2. add a debug-only, `recovery_fixture_enabled()`-gated protocol request that reads through `EventStore` and `ToolProgramNotificationService`;
3. use narrowly scoped SQL in the integration test only when the catalog layout cannot be reached through existing stores.

The inspection result must expose enough information to assert:

```text
event_count
stored event variant and semantic fields
notification state
injected_event_id
delivered_at
claim owner / lease state when relevant
```

Do not infer notification delivery from the parent job state or from a recovery report counter.

## 5. Work packages

### Work package A — Add failing semantic-confirmation tests

Add tests before the production fix covering:

1. Claimed notification plus matching existing event reaches Delivered.
2. Claimed notification plus same ID/different content remains unmarked and unacknowledged.
3. Claimed notification plus same ID/different notification ID fails closed.
4. Claimed notification plus wrong event variant fails closed.
5. Claimed notification plus malformed stored payload fails closed.
6. Claimed notification plus event-store query failure reports a typed error and does not become `leased` success.
7. Already-injected notification with a missing durable event does not acknowledge.
8. Already-injected notification with a mismatched injected event ID does not acknowledge.
9. Pending notification append collision does not mark or acknowledge.

Use direct state assertions on the durable notification record after every error case.

### Work package B — Centralize event reconstruction and comparison

Implement the expected-event helper and one shared semantic comparison path.

Required code properties:

- one formatter produces notification content for initial injection and restart recovery;
- `append_idempotent` and `confirm_existing` share semantic comparison logic;
- timestamp is the only intentionally ignored event field;
- event ID, session ID, event variant, injection key, notification ID, program ID, and content are mandatory matches;
- malformed stored JSON is never treated as absence;
- query errors remain `Err`.

### Work package C — Correct recovery transitions

Refactor `src/agent/tool_program_recovery.rs` to implement the state machine in Section 4.3.

The report should distinguish at least:

- inserted and delivered;
- confirmed existing event and delivered;
- already injected and acknowledged;
- leased with no event;
- semantic collision;
- storage/query error;
- CAS/notification transition error.

Do not count collision or storage failure as `skipped`, `leased`, or successful recovery without also returning a typed error entry.

### Work package D — Strengthen real process evidence

Create or extend `tests/tool_program_m017_notification_confirmation.rs`.

The nominal crash fixture must:

1. create a real background Tool Program notification in durable state;
2. start the failpoint process and reach `tool_program_after_session_append`;
3. kill that process before `mark_injected`;
4. start a fresh recovery process against the same catalog;
5. recover through the fixture-gated production boundary;
6. inspect durable state directly;
7. assert exactly one matching session event;
8. assert the event has the expected session, injection key, notification ID, program ID, and content;
9. assert notification state is Delivered;
10. assert `injected_event_id` equals the stable event ID;
11. assert `delivered_at` is present;
12. terminate the recovery process;
13. start a third fresh process against the same catalog;
14. recover and inspect again;
15. assert no second event, no state regression, and no new delivery transition.

Add a semantic-collision process or store-level fixture that seeds the same stable event ID with different content and proves recovery does not mark or acknowledge the notification.

The test must fail on process-start failure, marker timeout, inspection failure, missing rows, malformed rows, or unexpected state. No skips are allowed.

### Work package E — Documentation and independent closure

After code and tests land:

- move M017 to `closing`;
- leave `plans/closure/tool-programs/017-status.md` absent;
- record the exact implementation head and command evidence;
- update `plans/subsystems/tool-programs-roadmap.md` so its header, completion definition, and milestone table include M016 as historical conditional and M017 as closing;
- update the closure addendum and registry consistently;
- retain M015 and M016 as historical conditional records.

A separate reviewer must then:

- inspect the exact implementation head;
- rerun M017, M016, and affected M015 tests;
- verify the durable state assertions rather than relying on commit prose;
- verify query failures and semantic collisions cannot advance notification state;
- create `plans/closure/tool-programs/017-status.md` in a later commit only if all criteria pass;
- synchronize roadmap, addendum, M017 plan, closure records, architecture documentation, and registry to `closed`.

## 6. Required commit sequence

Use separate commits in this order:

1. `test(tool-programs): expose existence-only notification recovery defect`
2. `test(tool-programs): require typed event-store error propagation`
3. `fix(tool-programs): add semantic existing-event confirmation`
4. `fix(tool-programs): enforce confirmed notification recovery transitions`
5. `test(tool-programs): prove durable delivery across three processes`
6. `test(tool-programs): reject persisted semantic collision during recovery`
7. `docs(plans): move Tool Programs M017 to closing`

Do not squash implementation and independent closure into one commit. The implementation agent must not create or approve `017-status.md`.

## 7. Required verification

Run with bounded concurrency:

```text
cargo fmt --all -- --check
cargo check -p codegg-core
cargo check -p codegg --all-features
cargo clippy -p codegg-core --all-targets -- -D warnings
cargo clippy -p codegg --all-targets --all-features -- -D warnings
cargo test -p codegg-core event_store_idempotency_tests -- --test-threads=1
cargo test -p codegg-core semantic_equals -- --test-threads=1
cargo test -p codegg tool_program_recovery --lib -- --test-threads=1
cargo test -p codegg --test tool_program_m017_notification_confirmation -- --test-threads=1
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

If package-targeted Clippy exposes a pre-existing warning outside changed Tool Programs files, record the exact file and lint. Any warning in changed M017 files is a closure blocker.

## 8. Binary acceptance criteria

### Semantic confirmation

- C-01: production recovery does not use event existence as semantic confirmation.
- C-02: `has_event(...).unwrap_or(false)` or an equivalent error-swallowing pattern is absent from notification recovery.
- C-03: expected event reconstruction is shared by first delivery and restart recovery.
- C-04: the stable event ID remains `tp-event:{injection_key}`.
- C-05: timestamp-only differences are accepted.
- C-06: event-ID mismatch fails closed.
- C-07: session-ID mismatch fails closed.
- C-08: event-variant mismatch fails closed.
- C-09: injection-key mismatch fails closed.
- C-10: notification-ID mismatch fails closed.
- C-11: program-ID mismatch fails closed.
- C-12: content mismatch fails closed.
- C-13: malformed stored payload fails closed.
- C-14: event-store query failure is propagated as a typed recovery error.
- C-15: a semantic match does not update, delete, or replace the stored event.

### Recovery transitions

- C-16: an already-injected notification is acknowledged only after stable ID and durable event confirmation.
- C-17: an already-injected notification with a missing event is not acknowledged.
- C-18: a Claimed notification with a matching event is marked and acknowledged.
- C-19: a Claimed notification with no event remains claimed for lease expiry and does not insert an event.
- C-20: a Claimed notification with a collision is neither marked nor acknowledged.
- C-21: a Claimed notification with a query error is neither marked nor acknowledged.
- C-22: a Pending notification claims before event insertion.
- C-23: a Pending notification marks only after append or semantic reconciliation succeeds.
- C-24: acknowledgement occurs only after the stable injected identity is durable.
- C-25: append, mark, and acknowledgement errors remain separately observable.
- C-26: repeated recovery converges without duplicate events or duplicate delivery.

### Durable process evidence

- C-27: the failpoint process reaches the append-before-mark marker before termination.
- C-28: the recovery process shares only durable catalog/workspace state with the failed process.
- C-29: durable inspection reports exactly one parent-session notification event.
- C-30: the stored event’s semantic fields match the durable notification.
- C-31: the durable notification state is Delivered.
- C-32: `injected_event_id` equals the stable event ID.
- C-33: `delivered_at` is present.
- C-34: a third process restart leaves event count, event content, injected ID, and Delivered state unchanged.
- C-35: a persisted semantic collision prevents mark and acknowledgement.
- C-36: process startup, marker, query, and assertion failures fail the test rather than skip.

### Regression and governance

- C-37: affected M015 and M016 notification/process suites pass.
- C-38: broader Tool Program library/core tests pass with bounded concurrency.
- C-39: native harness and static architecture guards pass.
- C-40: no accepted-decision, contract, replay, child, artifact, descendant, scheduler, or native-only invariant is weakened.
- C-41: M016 has a conditional closure record that names the transferred findings.
- C-42: M017 implementation moves only to `closing` and leaves `017-status.md` absent.
- C-43: a separate reviewer records the exact implementation head and independently reruns the required suites.
- C-44: canonical roadmap, addendum, M017 plan, closure records, architecture documentation, and registry agree before strict closure.
- C-45: no unresolved high or medium Tool Programs finding remains.

All C-01 through C-45 are mandatory.

## 9. Rejected shortcuts

The following do not satisfy M017:

- checking only whether an event ID exists;
- calling `unwrap_or(false)`, `unwrap_or_default`, or equivalent on an event-store recovery query;
- marking injected before deserializing and semantically comparing an existing event;
- accepting a same-ID event with different content or lineage;
- inserting an event for a Claimed notification while another owner’s lease may be live;
- treating a query failure as `Absent` or `leased`;
- treating Tool Program job completion as proof that the notification is Delivered;
- asserting only recovery report counters without inspecting durable rows;
- omitting the additional fresh-process restart;
- using shared in-memory objects across process phases;
- adding a fixture inspection request without gating it to the existing debug recovery capability;
- claiming closure while the canonical roadmap still reports M015 as the active final milestone;
- self-creating or self-approving `017-status.md` in the implementation pass.

## 10. Independent closure checklist

The independent reviewer must answer all of the following with code and test evidence:

1. Where is the expected notification event reconstructed, and do all branches use the same helper?
2. Which API loads and semantically confirms an existing event?
3. Can any event-store query error become `false`, `Absent`, `leased`, or success?
4. Can a same-ID event with different content advance `mark_injected` or `acknowledge`?
5. Does the Claimed/no-event branch avoid inserting while the lease may be live?
6. Does the process test inspect the durable event and notification rows directly?
7. Does the test prove one event, stable injected ID, Delivered state, and delivered timestamp?
8. Does a third fresh process prove stable duplicate-free recovery?
9. Did the reviewer independently rerun all required bounded tests and guards at the exact implementation head?
10. Are the roadmap, addendum, M017 plan, M016/M017 closure records, architecture docs, and registry synchronized?

Strict native-only Tool Programs closure may be restored only after all ten answers are affirmative and C-01 through C-45 are accepted.

No additional corrective milestone should be registered after M017 unless a new production-path defect is demonstrated.
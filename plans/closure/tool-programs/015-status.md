# Tool Programs Milestone 015 — Closure Status

Status: historical conditionally closed — implementation and independent-review record; strict closure transferred to M016

Source implementation plan:

- `plans/implementation/tool-programs/015-final-production-path-and-independent-closure.md`

Reviewed implementation head:

- `247ef5015d79bdd834bffca15c76ebb2426beb40`

Original independent review evidence:

- `230f435fa03fb7464607f0b4cf9e4be239621701`
- `plans/closure/tool-programs/015-independent-review.md`

Post-closure review baseline:

- `9bd9d0bf1e27a021e5610fb8564ca601fda775c0`

Successor strict-closure plan:

- `plans/implementation/tool-programs/016-notification-replay-polish-and-final-closure.md`

## 1. Corrected disposition

M015 landed substantial and valuable production corrections and remains the implementation foundation for strict native-only Tool Programs closure.

A post-closure production-path review found one narrow restart-correctness defect in parent-session notification injection. That finding invalidates the strict M015 claim that the append-before-mark crash window always converges to an acknowledged exactly-once notification. It does not invalidate M015 authority, contract, replay, child, artifact, descendant, process-fixture, or native-only work.

M015 is therefore retained as a historical, conditionally closed implementation and review record. Strict closure is transferred exclusively to M016.

## 2. Retained M015 implementation value

M015 materially corrected:

- accepted-decision authority at the normal Tool Program submission boundary;
- rejection of arbitrary public `JobSubmit` Tool Program authority fabrication;
- one frozen runtime Broker contract snapshot and canonical digest from submission through nested calls;
- monotonic checkpoint and completed-call recovery;
- active-child persistence, reattachment, and original-deadline verification;
- canonical call, child, result, and large-output artifact handling;
- typed notification storage failures and durable append-before-mark ordering;
- traversal through terminal intermediate descendants and resource convergence;
- real debug process-owner failpoint fixtures across two daemon processes;
- separation of implementation, independent review, and closure commits.

These mechanisms and their regression suites are the baseline for M016 and must not be reverted without a failing production-path test.

## 3. Post-M015 finding

### F01 — Reconstructed notification event collides after append-before-mark crash

The parent-session notification event uses a stable event ID derived from the durable injection key. However, each delivery attempt reconstructs the event with a new `created_at` timestamp.

`EventStore::append_idempotent` accepts an existing event only when the complete serialized event payload matches. After process A appends the event and crashes before `mark_injected`, process B can reconstruct the same logical event ID with a different timestamp. The conflict path then treats the retry as an identity/content collision.

Consequences:

- the existing session event is not duplicated;
- the notification may remain unmarked and unacknowledged;
- repeated recovery can fail indefinitely on the same collision;
- the append-before-mark process test does not prove eventual delivered-state convergence for a genuinely reconstructed event.

The existing idempotency unit test reuses the same fixed event instance and therefore does not reproduce this production restart path.

Transferred to M016 work packages A through D and criteria C-01 through C-29.

## 4. Corrected M015 criteria disposition

The original M015 C-01 through C-52 strict-closure claim is corrected as follows:

- accepted-decision authority and public-protocol rejection: retained;
- canonical contract snapshot convergence: retained;
- monotonic call/checkpoint recovery: retained;
- active-child persistence and reattachment: retained;
- canonical artifacts and result integrity: retained;
- typed fail-closed notification storage operations: substantially retained;
- durable append-before-mark ordering: retained;
- notification reconstruction and eventual acknowledgement after append-before-mark crash: not closed;
- descendant and resource convergence: retained;
- real daemon failpoint framework: retained;
- independent closure governance: structurally improved and retained;
- strict subsystem closure: transferred to M016.

## 5. Evidence disposition

The M015 implementation and independent-review records report successful focused Tool Program tests, broader library/core tests, the native harness, and static guards. No GitHub workflow or combined status checks were attached to the closed head.

That evidence remains useful regression coverage. M016 must rerun the affected notification and daemon suites and add a mechanism-faithful process test in which process B reconstructs the notification event from durable state after process A has appended it.

## 6. Final status

M015 is conditionally closed as a historical implementation and independent-review record.

Strict native-only Tool Programs closure is owned exclusively by:

- `plans/implementation/tool-programs/016-notification-replay-polish-and-final-closure.md`

M016 must remain `ready` or `closing` until a separate reviewer creates and accepts:

- `plans/closure/tool-programs/016-status.md`

No document should claim strict exactly-once parent-session notification closure until all M016 C-01 through C-37 criteria are independently verified at the exact reviewed implementation head.
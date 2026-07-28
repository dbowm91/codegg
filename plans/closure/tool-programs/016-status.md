# Tool Programs Milestone 016 — Closure Status

Status: conditionally closed — implementation retained; strict closure transferred to M017

Source implementation plan:

- `plans/implementation/tool-programs/016-notification-replay-polish-and-final-closure.md`

Implementation head reviewed:

- `f4101b9cb739889f65c92aa747e6869d49241c88`

Successor strict-closure plan:

- `plans/implementation/tool-programs/017-semantic-recovery-confirmation-and-evidence-closure.md`

## 1. Executive finding

M016 materially fixes the timestamp-only collision that prevented a reconstructed `ToolProgramNotificationEvent` from matching an already appended event after restart. The new semantic equality compares event ID, session, injection key, notification ID, program ID, and content while intentionally excluding `created_at`. `EventStore::append_idempotent` uses that comparison for Tool Program notification conflicts.

M016 is not strictly closed. The recovery loop bypasses semantic confirmation when an event ID already exists, swallows event-store lookup errors as absence, and does not yet provide direct durable-state evidence for all process-level closure claims.

The implementation is retained as the foundation for M017. Strict native-only Tool Programs closure transfers exclusively to M017.

## 2. Retained implementation value

M016 added and retained:

- `ToolProgramNotificationEvent::semantic_equals` with timestamp-only exclusion;
- fail-closed semantic collision handling inside `EventStore::append_idempotent`;
- unit coverage for timestamp reconstruction and semantic field mismatches;
- a reusable notification recovery loop;
- Pending and Claimed notification retrieval;
- a fixture-gated daemon recovery request;
- a real process append/failpoint/kill/restart fixture;
- planning-state movement from ready to closing without self-creating a closure record.

These mechanisms must not be removed by M017. M017 corrects how the recovery loop consumes them and strengthens durable evidence.

## 3. Unresolved findings

### F01 — Existing event ID is accepted without semantic confirmation

`inject_recoverable_notifications` checks `EventStore::has_event(event_id)`. If the ID exists, it calls `mark_injected` and `acknowledge` without loading the stored event or comparing its session, variant, injection key, notification ID, program ID, or content to the expected event.

Severity: medium.

Impact: a malformed or semantically conflicting event that reuses the stable ID can advance the notification to Delivered even though `append_idempotent` would reject the same collision.

Transferred to M017 F01, work packages A–C, and criteria C-01 through C-21.

### F02 — Event-store query failure is converted to absence

The same branch uses `has_event(...).await.unwrap_or(false)`.

Severity: medium.

Impact: a storage/query failure can be misclassified as no event. Claimed work may be reported as leased and Pending work may continue toward insertion rather than surfacing a typed failure.

Transferred to M017 F02 and criteria C-02, C-14, C-21, and C-25.

### F03 — Process evidence does not directly prove durable delivery state

The M016 process fixture verifies recovery report counters and the Tool Program job’s completed state. It does not directly assert:

- the exact persisted session-event count;
- the stored event’s semantic fields;
- notification `state = delivered`;
- the stable `injected_event_id`;
- a non-null `delivered_at`;
- stable state after an additional fresh-process restart.

Severity: medium evidence/correctness gap.

Impact: the fixture can pass without proving the complete notification persistence contract.

Transferred to M017 F03, work package D, and criteria C-27 through C-36.

### F04 — Canonical planning documents disagree

The registry and correctness addendum show M016 closing, while `plans/subsystems/tool-programs-roadmap.md` still reports the subsystem closed at M015 and has no M016/M017 milestone disposition.

Severity: medium governance/evidence gap.

Impact: strict closure cannot be audited from one internally consistent planning state.

Transferred to M017 F04, work package E, and criteria C-41 through C-45.

## 4. Requirement disposition

The original M016 C-01 through C-37 criteria are corrected as follows:

- stable event ID: retained;
- timestamp-only semantic equality: retained;
- append conflict semantic validation: retained;
- semantic collision rejection inside `append_idempotent`: retained;
- marking only after semantic confirmation in every recovery branch: not closed;
- typed event-store query error propagation in recovery: not closed;
- Claimed/no-event lease handling: partially implemented;
- exact durable event count and notification state evidence: not closed;
- additional restart stability evidence: not closed;
- independent strict closure: not accepted;
- roadmap/addendum/registry agreement: not closed.

## 5. Evidence reviewed

The implementation commit reports successful focused M016, affected M015, broader Tool Program, native harness, and static-guard tests. No GitHub workflow runs or combined status checks are attached to `f4101b9`.

This review accepts the landed mechanisms described in Section 2 but does not treat commit prose or recovery report counters as sufficient evidence for the unresolved findings.

## 6. Final disposition

M016 is conditionally closed as an implementation milestone.

Strict native-only Tool Programs closure is owned exclusively by:

- `plans/implementation/tool-programs/017-semantic-recovery-confirmation-and-evidence-closure.md`

The M017 implementation pass must leave this absent:

- `plans/closure/tool-programs/017-status.md`

A separate reviewer may create that record only after every M017 C-01 through C-45 criterion passes at the exact reviewed implementation head.
# Tool Programs Correctness and Ownership Closure Addendum

Status: active — Milestone 016 ready for implementation

Canonical subsystem roadmap:

- `plans/subsystems/tool-programs-roadmap.md`

Current strict-closure implementation plan:

- `plans/implementation/tool-programs/016-notification-replay-polish-and-final-closure.md`

Historical predecessors:

- `plans/implementation/tool-programs/015-final-production-path-and-independent-closure.md`
- `plans/closure/tool-programs/015-status.md`
- `plans/implementation/tool-programs/014-production-boundary-and-process-evidence-closure.md`
- `plans/closure/tool-programs/014-status.md`
- `plans/implementation/tool-programs/013-production-authority-descendant-and-recovery-closure.md`
- `plans/closure/tool-programs/013-status.md`
- `plans/implementation/tool-programs/012-authority-recovery-and-delivery-corrective-closure.md`
- `plans/closure/tool-programs/012-status.md`
- `plans/implementation/tool-programs/011-production-correctness-and-ownership-closure.md`
- `plans/closure/tool-programs/011-status.md`

M016 baseline reviewed:

- `9bd9d0bf1e27a021e5610fb8564ca601fda775c0` (`main`)

Applicable ADR:

- `plans/adrs/ADR-0001-programmatic-tool-execution-authority.md`

## 1. Purpose

This addendum is the active corrective control document for strict native-only Tool Programs closure.

M015 closed the broad production gaps carried forward from M011–M014: accepted-decision authority, one runtime contract snapshot, monotonic replay, active-child restart recovery, canonical artifacts, typed notification persistence, complete descendant traversal, real daemon failpoints, and separate implementation/review governance.

A post-M015 production-path review demonstrated one remaining restart defect. Parent-session notification events use a stable event ID, but process B reconstructs the event with a new timestamp after a crash between session append and `mark_injected`. The current idempotent append compares complete serialized payloads, so the same logical event can be rejected as an identity collision and the notification can remain permanently unacknowledged.

M015 is therefore retained as a historical conditionally closed implementation and review record. M016 is the sole active strict-closure authority.

M016 is a narrow polish pass. It does not redesign Tool Programs, broaden authority, change the restricted Python language, expand the programmable palette, or implement hosted execution.

## 2. Post-M015 trigger finding

The review of `9bd9d0bf1e27a021e5610fb8564ca601fda775c0` found:

1. `ToolProgramNotificationEvent` uses stable event identity but reconstructs `created_at` on each delivery attempt;
2. `EventStore::append_idempotent` accepts an existing event only when the complete serialized payload matches;
3. a crash after append and before `mark_injected` can therefore make process B reject the same logical event because only timestamp metadata changed;
4. the existing unit test reuses one fixed event instance and does not reproduce a reconstructed process-B event;
5. the current process evidence does not prove that this exact crash window reaches injected and delivered state without a duplicate event.

This is a narrow medium restart-correctness defect. It invalidates strict exactly-once parent-session notification closure but does not invalidate the other M015 mechanisms.

## 3. Current corrective milestone

### Milestone 016 — Notification replay polish and final closure

Class: narrow restart-correctness polish / durable session injection / independent closure

Objective: make reconstruction of one logical Tool Program notification event semantically idempotent across the append-before-mark crash window, prove eventual delivered-state convergence through a real two-process fixture, and restore strict closure through a separate reviewer.

Dependencies:

- M001–M015 implementation is present;
- M015 accepted-decision authority, contract, replay, child, artifact, descendant, process-fixture, and native-only mechanisms remain the baseline;
- the durable notification record, session event store, SQLite notification service, AgentLoop delivery path, and debug failpoint harness are available;
- no external provider is required;
- production remains explicitly `native_only`;
- M016 does not broaden program authority.

Exit conditions:

- `tp-event:{injection_key}` remains the stable parent-session event ID;
- timestamp-only reconstruction differences do not cause a collision for the same logical notification;
- session, injection key, notification, program, event type, and exact content differences still fail closed;
- an existing matching event is accepted without update, delete, or duplication;
- delivery order remains append/confirm, mark injected, then acknowledge;
- append, mark, acknowledgement, query, and serialization failures remain typed and recoverable;
- a real process-A append/failpoint/kill and process-B reconstruction reaches one event and delivered state;
- repeated restart remains stable and duplicate-free;
- affected M015 suites, broader bounded Tool Program tests, native harness, and static guards pass;
- implementation moves only to `closing`; a separate reviewer later creates and accepts the M016 closure record;
- no unresolved high or medium finding remains.

## 4. Dependency graph

```text
M001–M014 foundations and corrective implementations
        |
        v
M015 final production-path implementation
(conditionally closed after notification replay review)
        |
        v
M016 notification replay polish and independent closure
        |
        v
Strict native-only Tool Programs subsystem closure
```

Historical records remain traceability artifacts and must not be rewritten to conceal their original claims. Their reconciled closure records identify corrected dispositions and successor criteria.

## 5. Closure authority

Until M016 closes:

- the Tool Programs subsystem status is `active`;
- M011 through M015 are historical implementation or review records, with M011–M015 conditionally closed where later production findings transferred strict closure forward;
- M016 is the sole active strict-closure authority;
- mutation-capable, destructive, approval-sensitive, shell, patch, Git mutation, commit, push, and subagent tools remain prohibited from the programmable palette;
- production remains `native_only`;
- documentation may not claim strict append-before-mark notification convergence or strict subsystem closure without M016 evidence.

The authoritative implementation and acceptance details are in:

- `plans/implementation/tool-programs/016-notification-replay-polish-and-final-closure.md`

The eventual independent closure record must be created at:

- `plans/closure/tool-programs/016-status.md`

The implementation pass must leave that closure record absent.

## 6. Milestone status

| Milestone | Status | Disposition |
|---|---|---|
| 001–009 | historical closed/capability records | Foundations retained; later milestones own production-boundary depth |
| 010 | conditionally closed, historical | Native harness retained; later corrective milestones own strict closure |
| 011 | conditionally closed, historical | Production ownership mechanics landed; strict closure transferred forward |
| 012 | conditionally closed, historical | Broker failure and native-only improvements retained; strict closure transferred forward |
| 013 | conditionally closed, historical | Grant persistence, CAS syntax, basic lineage, replay/result improvements retained; strict closure transferred forward |
| 014 | conditionally closed, historical | Checkpoint loading, v35 lineage, result integrity, native-only enforcement, and other improvements retained; strict closure transferred forward |
| 015 | conditionally closed, historical | Broad production-path closure retained; notification reconstruction defect transferred to M016 |
| 016 | ready | Sole active handoff for semantic notification replay and independent final closure |

## 7. Completion definition

The Tool Programs subsystem becomes strictly closed only when:

1. all M016 C-01 through C-37 criteria are true in production paths;
2. the same logical notification event can be reconstructed after restart without timestamp-only collision;
3. any different semantic event reusing the identity fails closed;
4. append or semantic confirmation precedes mark-injected, which precedes acknowledgement;
5. process A and process B share only durable state in the append-before-mark fixture;
6. one and only one parent-session event exists after recovery;
7. the notification reaches and remains in delivered state after repeated restart;
8. notification persistence, query, serialization, and conflict failures remain typed;
9. M015 authority, contract, replay, child, artifact, descendant, and native-only invariants remain green;
10. required formatting, compilation, targeted tests, native harness, and static guards pass;
11. the implementation pass leaves M016 at `closing` and does not create or approve its closure record;
12. a separate reviewer creates `plans/closure/tool-programs/016-status.md` at a later commit with exact implementation head and independent evidence;
13. no unresolved high or medium finding remains;
14. roadmap, addendum, implementation plan, closure records, architecture documentation, and registry agree.

No additional corrective milestone should be registered after M016 unless a new production-path defect is demonstrated.
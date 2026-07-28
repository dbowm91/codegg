# Tool Programs Correctness and Ownership Closure Addendum

Status: closing — Milestone 017 implementation landed, awaiting independent reviewer

Canonical subsystem roadmap:

- `plans/subsystems/tool-programs-roadmap.md`

Current strict-closure implementation plan:

- `plans/implementation/tool-programs/017-semantic-recovery-confirmation-and-evidence-closure.md`

Current conditional review record:

- `plans/closure/tool-programs/016-status.md`

Historical predecessors:

- `plans/implementation/tool-programs/016-notification-replay-polish-and-final-closure.md`
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

M017 baseline reviewed:

- `f4101b9cb739889f65c92aa747e6869d49241c88` (`main`)

Applicable ADR:

- `plans/adrs/ADR-0001-programmatic-tool-execution-authority.md`

## 1. Purpose

This addendum is the active corrective control document for strict native-only Tool Programs closure.

M015 closed the broad production gaps carried forward from M011–M014: accepted-decision authority, one runtime contract snapshot, monotonic replay, active-child restart recovery, canonical artifacts, typed notification persistence, complete descendant traversal, real daemon failpoints, and separate implementation/review governance.

M016 fixed the timestamp-only collision in reconstructed parent-session notification events. It added semantic equality, integrated that equality into `EventStore::append_idempotent`, extracted a recovery loop, and added a real append/failpoint/restart fixture.

A post-M016 production-path review found that the recovery loop accepts event existence as semantic confirmation and converts event-store lookup failure to absence. The process fixture also does not directly prove the durable event and notification rows or stability after another restart. M016 is therefore conditionally closed as an implementation milestone.

M017 is the sole active strict-closure authority. It is a narrow final corrective pass. It does not redesign Tool Programs, broaden authority, change the restricted Python language, expand the programmable palette, or implement hosted execution.

## 2. Post-M016 trigger findings

The review of `f4101b9cb739889f65c92aa747e6869d49241c88` found:

1. `inject_recoverable_notifications` calls `has_event(event_id)` and marks/acknowledges on ID existence without loading or semantically comparing the stored event;
2. the same branch uses `unwrap_or(false)`, converting query/storage failure into event absence;
3. already-injected recovery does not first prove that the stable injected identity resolves to the expected durable event;
4. the process fixture checks recovery report counters and Tool Program job completion rather than direct notification `Delivered` state, stable `injected_event_id`, `delivered_at`, exact event count, and semantic event contents;
5. the fixture does not perform the required additional fresh-process restart;
6. the canonical roadmap still reports strict closure at M015 and has no M016/M017 disposition.

These are medium recovery-integrity, error-propagation, process-evidence, and governance defects. They invalidate strict M016 closure but do not erase M016’s semantic equality and restart-fixture value.

## 3. Current corrective milestone

### Milestone 017 — Semantic recovery confirmation and evidence closure

Class: final corrective implementation / semantic recovery confirmation / typed persistence errors / durable process evidence / independent closure

Objective: require semantic confirmation before every notification delivery state transition, propagate event-store failures without reinterpretation, directly prove durable event and notification state across three processes, synchronize planning documents, and restore strict closure through a separate reviewer.

Dependencies:

- M001–M016 implementation is present;
- M015 accepted-decision, contract, replay, child, artifact, descendant, scheduler, process-fixture, and native-only mechanisms remain the baseline;
- M016 `semantic_equals`, `append_idempotent`, notification recovery loop, fixture-gated recovery request, and process test are present;
- the durable notification store and session event store are available;
- no external provider is required;
- production remains explicitly `native_only`;
- M017 does not broaden program authority.

Exit conditions:

- one expected notification event is reconstructed from durable notification data in every branch;
- an existing event is loaded and semantically confirmed before `mark_injected` or `acknowledge`;
- event existence alone cannot advance notification state;
- query, deserialization, and collision failures remain typed errors;
- Claimed notifications with no event remain leased and do not insert while another owner may be live;
- Pending notifications claim before append, append before mark, and mark before acknowledgement;
- already-injected notifications are acknowledged only after stable-ID and durable-event confirmation;
- a real failpoint process, recovery process, and third verification process prove one event and stable Delivered state;
- direct durable inspection proves event count, semantic fields, notification state, injected event ID, and delivered timestamp;
- affected M015/M016 suites, broader bounded Tool Program tests, native harness, package-targeted Clippy, and static guards pass;
- the canonical roadmap, addendum, implementation plan, closure records, architecture documentation, and registry agree;
- implementation moves only to `closing`; a separate reviewer later creates and accepts the M017 closure record;
- no unresolved high or medium finding remains.

## 4. Dependency graph

```text
M001–M014 foundations and corrective implementations
        |
        v
M015 broad production-path implementation
(conditionally closed historical record)
        |
        v
M016 semantic event reconstruction implementation
(conditionally closed after recovery-path review)
        |
        v
M017 semantic recovery confirmation and evidence closure
        |
        v
Strict native-only Tool Programs subsystem closure
```

Historical records remain traceability artifacts and must not be rewritten to conceal their original claims. Their closure records identify corrected dispositions and successor criteria.

## 5. Closure authority

Until M017 closes:

- the Tool Programs subsystem status is `active`;
- M011 through M016 remain historical implementation or review records, conditionally closed where later production findings transferred strict closure forward;
- M017 is the sole active strict-closure authority;
- mutation-capable, destructive, approval-sensitive, shell, patch, Git mutation, commit, push, and subagent tools remain prohibited from the programmable palette;
- production remains `native_only`;
- documentation may not claim strict notification delivery closure or strict subsystem closure without M017 evidence.

The authoritative implementation and acceptance details are in:

- `plans/implementation/tool-programs/017-semantic-recovery-confirmation-and-evidence-closure.md`

The eventual independent closure record must be created at:

- `plans/closure/tool-programs/017-status.md`

The implementation pass must leave that closure record absent.

## 6. Milestone status

| Milestone | Status | Disposition |
|---|---|---|
| 001–009 | historical closed/capability records | Foundations retained; later milestones own production-boundary depth |
| 010 | conditionally closed, historical | Native harness retained; later corrective milestones own strict closure |
| 011 | conditionally closed, historical | Production ownership mechanics landed; strict closure transferred forward |
| 012 | conditionally closed, historical | Broker failure and native-only improvements retained; strict closure transferred forward |
| 013 | conditionally closed, historical | Grant persistence, CAS syntax, lineage, replay, and result improvements retained; strict closure transferred forward |
| 014 | conditionally closed, historical | Checkpoint loading, v35 lineage, result integrity, native-only enforcement, and other improvements retained; strict closure transferred forward |
| 015 | conditionally closed, historical | Broad production-path closure retained; notification reconstruction defect transferred forward |
| 016 | conditionally closed, historical | Semantic event equality and restart fixture retained; recovery confirmation/evidence defects transferred to M017 |
| 017 | closing | Sole active handoff for semantic confirmation, typed errors, direct durable evidence, document convergence, and independent final closure |

## 7. Completion definition

The Tool Programs subsystem becomes strictly closed only when:

1. all M017 C-01 through C-45 criteria are true in production paths;
2. no notification state transition relies on event-ID existence alone;
3. an existing event is deserialized and semantically matched before mark or acknowledgement;
4. no event-store error can become absence, leased state, skipped success, or silent continuation;
5. Claimed/no-event recovery does not insert while another owner’s lease may be active;
6. Pending recovery preserves claim, append, mark, acknowledge ordering;
7. already-injected recovery confirms the stable durable event before acknowledgement;
8. direct durable inspection proves exactly one event and a Delivered notification with stable injected ID and delivered timestamp;
9. a third fresh process proves stable duplicate-free recovery;
10. M015 and M016 authority, contract, replay, child, artifact, descendant, scheduler, and native-only invariants remain green;
11. required formatting, compilation, package-targeted Clippy, targeted tests, native harness, and static guards pass;
12. the implementation pass leaves M017 at `closing` and does not create or approve its closure record;
13. a separate reviewer creates `plans/closure/tool-programs/017-status.md` at a later commit with exact implementation head and independent evidence;
14. no unresolved high or medium finding remains;
15. roadmap, addendum, implementation plan, closure records, architecture documentation, and registry agree.

No additional corrective milestone should be registered after M017 unless a new production-path defect is demonstrated.
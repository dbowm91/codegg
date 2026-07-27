# Tool Programs Correctness and Ownership Closure Addendum

Status: closing — Milestone 013 implementation landed, closure review pending; Milestone 012 is historical conditionally closed; Milestone 011 is historical conditionally closed

Canonical subsystem roadmap:

- `plans/subsystems/tool-programs-roadmap.md`

Current corrective implementation plan:

- `plans/implementation/tool-programs/013-production-authority-descendant-and-recovery-closure.md`

Historical predecessor:

- `plans/implementation/tool-programs/011-production-correctness-and-ownership-closure.md`
- `plans/closure/tool-programs/011-status.md`

Post-M011 baseline reviewed:

- `d71a5eee5b31876545981fdb0bd8e437aadee39c` (`main`)

Applicable ADR:

- `plans/adrs/ADR-0001-programmatic-tool-execution-authority.md`

## 1. Purpose

This addendum remains the active corrective control document for strict Tool Programs closure.

M011 added substantial production-path mechanics, including distinct invocation identity, Broker routing, scheduler timeout/heartbeat plumbing, call journaling, typed results, child sequence identity, and SQLite-backed notifications. A post-closure review found that several ownership claims were stronger than the actual mechanisms and tests. M011 is therefore retained as a historical, conditionally closed implementation record rather than the current strict-closure authority.

M012 owns the remaining correctness work. It is a bounded corrective pass, not a subsystem redesign and not an expansion of the version-1 programmable palette.

## 2. Post-M011 trigger findings

The review of `d71a5eee5b31876545981fdb0bd8e437aadee39c` found:

1. production Tool Program authority is synthesized from constants/digests rather than a scope-verifiable permission decision;
2. the Broker can return an error-valued `Ok(BrokerResult)`, and the program adapter can persist that as a successful completed call;
3. notification claim and acknowledgement are decided in process-local memory and later upserted, so concurrent service instances are not coordinated transactionally and storage failures can still report success;
4. scheduler timeout can drop the parent executor before its child-wait loop cancels descendants, while child jobs lack a canonical scheduler-queryable parent relationship;
5. checkpoints are persisted but not restored, and replay identity is not bound to the full authority/context/contract/manifest/workspace/control-flow fingerprint;
6. child jobs cannot be durably reattached through parent call identity after restart and return no real artifact handles;
7. typed result projections still emit empty program artifact lists and do not verify the stored result digest on load;
8. hosted policies are model-facing and selectable even though normal production runtime construction cannot execute the hosted adapter;
9. M011's dedicated tests are primarily component/store fixtures and do not establish process-level daemon restart, concurrent claim, child reattachment, or capacity-one convergence.

These findings include unresolved high and medium correctness, authorization, recovery, notification, child-ownership, resource, and evidence defects. They invalidate strict M011 closure but do not erase the implementation value or evidence recorded by M011.

## 3. Corrective milestone

### Milestone 012 — Authority, recovery, delivery, and child-ownership corrective closure

Class: invariant / correctness / authorization / recovery / scheduler ownership / final closure

Objective: correct the remaining production ownership defects, prove the mechanisms through public process-level and concurrent-service tests, and reconcile hosted execution truthfully.

Dependencies:

- M001–M010 implementation and historical records are present;
- M011 implementation is present and conditionally closed;
- scheduler, Tool Broker, restricted interpreter, job/session SQLite store, artifact store, RunStore, notification service, and provider adapter foundations are available;
- no external provider is a hard dependency for native correctness;
- the recommended hosted disposition is explicit native-only production status unless an existing narrow transport injection seam can satisfy the complete M012 hosted acceptance gate.

Exit conditions:

- real permission/path-policy decisions produce versioned, scope-verifiable authority grants;
- every direct and programmatic Broker call verifies the grant against caller, effect, tool/contract manifest, workspace/path policy, and policy revision;
- denied, failed, cancelled, timed-out, and schema-invalid nested calls cannot become successful completed calls;
- notification claim/reclaim/acknowledgement are SQLite compare-and-set transitions, persistence errors propagate, and append-before-ack restart produces one durable injection;
- child jobs have durable parent program/job/attempt/call identity and scheduler-owned descendant cancellation independent of executor-future cleanup;
- restart reattaches active children and never repeats a durably completed call;
- replay verifies full authority/context/contract/manifest/source/IR/workspace/backend/call-order identity and stops recoverably on divergence;
- the original absolute deadline remains authoritative across restart;
- typed results verify integrity and contain real, resolvable call/child/output artifact handles;
- production exposes only execution backends reachable through normal runtime construction;
- process-level daemon kill/restart, concurrent SQLite claimant, capacity-one child, descendant process cleanup, and result-corruption tests pass;
- no unresolved high or medium correctness, authorization, recovery, notification, child-ownership, resource, result-integrity, or evidence finding remains;
- an independent reviewer accepts `plans/closure/tool-programs/012-status.md`.

## 4. Dependency graph

```text
M001–M010 implementation and historical closure records
                         |
                         v
M011 production ownership implementation
(conditionally closed after post-closure review)
                         |
                         v
M012 authority/recovery/delivery corrective closure
                         |
                         v
Strict Tool Programs subsystem closure
```

M012 supersedes M011's strict closure claims only where the post-M011 findings apply. Historical closure records remain traceability artifacts and must not be rewritten to conceal their original evidence.

## 5. Closure authority

Until M012 closes:

- the subsystem status is `active`;
- M011 is `conditionally closed` and historical, not the current strict-closure authority;
- M002, M005, M007, M008, M009, M010, and M011 remain useful implementation/evidence records but do not independently establish production-boundary closure;
- mutation-capable, destructive, approval-sensitive, shell, patch, Git mutation, commit, push, and subagent tools remain prohibited from the programmable palette;
- documentation may not claim scope-verified program authority, transactionally exactly-once notification delivery, scheduler-owned descendant convergence, complete checkpoint/replay recovery, real child artifacts, or production hosted execution without M012 evidence.

The authoritative implementation and acceptance details are in:

- `plans/implementation/tool-programs/012-authority-recovery-and-delivery-corrective-closure.md`

The eventual closure record must be created at:

- `plans/closure/tool-programs/012-status.md`

## 6. Milestone status

| Milestone | Status | Disposition |
|---|---|---|
| 001 | historical closed | Scheduler-owned ordinary Python foundation retained |
| 002 | historical closed; revalidated by M011/M012 | Canonical Broker foundation retained; M012 owns real grant verification and failure semantics |
| 003 | historical closed | Durable domain/storage foundation retained and extended as needed |
| 004 | historical closed | Restricted-Python frontend and static bounds retained |
| 005 | historical closed; revalidated by M011/M012 | M012 owns authoritative recovery cursor, replay binding, and process-level restart proof |
| 006 | historical closed | Read-only programmable palette retained; no authority expansion |
| 007 | historical closed; revalidated by M011/M012 | M012 owns durable child lineage, reattachment, scheduler cancellation, permits, and artifacts |
| 008 | historical closed; revalidated by M011/M012 | M012 owns transactional notification claim/injection/acknowledgement and restart proof |
| 009 | historical closed; capability/library record | M012 must wire hosted production execution completely or classify it explicitly non-production |
| 010 | conditionally closed, historical | Native harness retained; strict closure transferred through M011 to M012 |
| 011 | conditionally closed, historical | Substantial production mechanics landed; post-closure high/medium findings are owned by M012 |
| 012 | closing | Authority, failure semantics, transactional delivery, descendant ownership, replay, artifacts, hosted truthfulness, and process-level closure |

## 7. Completion definition

The Tool Programs subsystem becomes strictly closed only when:

1. M012 has an independently accepted closure record;
2. the original roadmap invariants and M012 C-01 through C-32 are true in the production daemon path;
3. authority is derived from and verified against real permission/path-policy decisions;
4. non-success nested calls cannot be persisted or replayed as successful completions;
5. process-level restart never repeats a durably completed call or child submission;
6. session-addressed notification delivery is durable and exactly once across claim, injection, acknowledgement, concurrent service instances, and restart;
7. scheduler terminalization cancels and reconciles descendants independently of executor cleanup;
8. source, invocation, grant, attempt, call, child, run, result, artifact, and notification identities are distinct and correctly correlated;
9. foreground, background, notification, and inspection consume one integrity-checked typed result;
10. hosted production status is truthful and model-facing policy exposes no unreachable backend;
11. full targeted tests, process-level fault tests, migrations, formatting, compilation, and repository-owned static guards pass;
12. broader workspace failures are either fixed or documented with evidence that they are unrelated and cannot invalidate M012;
13. live operational evidence is labeled truthfully;
14. roadmap/addendum, implementation plan, closure record, architecture documentation, and registry agree.
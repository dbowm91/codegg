# Tool Programs Correctness and Ownership Closure Addendum

Status: active — Milestone 013 ready for implementation; Milestones 011 and 012 are historical conditionally closed records

Canonical subsystem roadmap:

- `plans/subsystems/tool-programs-roadmap.md`

Current corrective implementation plan:

- `plans/implementation/tool-programs/013-production-authority-descendant-and-recovery-closure.md`

Historical predecessors:

- `plans/implementation/tool-programs/012-authority-recovery-and-delivery-corrective-closure.md`
- `plans/closure/tool-programs/012-status.md`
- `plans/implementation/tool-programs/011-production-correctness-and-ownership-closure.md`
- `plans/closure/tool-programs/011-status.md`

Post-M012 baseline reviewed:

- `d056e4236e1ef10b4639b8bbf05557090dc6112c` (`main`)

Applicable ADR:

- `plans/adrs/ADR-0001-programmatic-tool-execution-authority.md`

## 1. Purpose

This addendum is the active corrective control document for strict Tool Programs closure.

M011 and M012 added substantial production mechanics: distinct invocation identity, Broker routing, typed non-success handling, scheduler timeout plumbing, call journaling, typed results, native-only model-facing policy, notification state APIs, and initial child-lineage fields. Those milestones remain useful implementation and evidence records.

A post-M012 production-path review found that several closure claims still exceed the implemented mechanisms and tests. M012 is therefore retained as a historical, conditionally closed implementation record. M013 is the sole active strict-closure authority.

M013 is a bounded corrective pass. It does not redesign the restricted Python frontend, expand the version-1 programmable palette, or implement hosted Tool Programs.

## 2. Post-M012 trigger findings

The review of `d056e4236e1ef10b4639b8bbf05557090dc6112c` found:

1. the authority grant is still synthesized from program/workspace/session/source/timestamp material inside the executor rather than created from and persisted as the real permission and workspace path-policy decision before admission;
2. the Broker checks that authority is represented by the `Verified` enum variant but does not verify the grant's integrity, expiry, revocation, principal, workspace, path policy, caller class, effect class, manifest, contract version, or policy revision against the actual invocation;
3. the SQLite notification transition query uses incorrect parameter/state handling, while closure tests exercise in-memory state or two references to one service instead of independent service instances sharing a real database;
4. child lineage fields are populated on `NewJob` but discarded by the stores, have no complete SQLite migration/query mapping, and use an operation-derived parent call value rather than canonical call identity and sequence;
5. scheduler timeout still drops the parent executor future before scheduler-owned recursive descendant cleanup, and there is no durable descendant enumeration, reattachment, lost-worker cleanup, or daemon-generation reconciliation based on lineage;
6. restart reloads completed calls but does not restore the checkpoint or bind replay to authority, context, workspace/path policy, manifest, contract, source/IR, backend, deadline, call-order, call-ID, and child identity;
7. the replay journal remains a concurrent whole-file read/modify/write store, retains more call data than its redaction comments imply, and emits MD5 values under a `sha256` label;
8. child artifacts remain empty, call artifact digests may be absent, and the result digest covers only `ProgramResult` rather than the complete semantic result record;
9. closure-bearing process tests do not start or kill a daemon, restart against the same database, use failpoints, coordinate independent SQLite services, reattach a child, or prove process-group and permit convergence;
10. the M012 closure record contains an incorrect implementation SHA and marks criteria as passing while also admitting required mechanisms are absent.

These are unresolved high and medium authorization, delivery, descendant-ownership, recovery, integrity, resource, and evidence defects. They invalidate strict M012 closure but do not erase the implementation value recorded by M012.

## 3. Current corrective milestone

### Milestone 013 — Production authority, descendant, delivery, and recovery closure

Class: corrective implementation / authorization / durable ownership / scheduler convergence / recovery / evidence closure

Objective: complete the missing production mechanisms, replace structural evidence with mechanism-faithful tests, and establish a reviewable strict-closure record.

Dependencies:

- M001–M012 implementation and historical records are present;
- scheduler, Tool Broker, restricted interpreter, job/session SQLite store, artifact store, RunStore, notification service, and native Tool Program runtime foundations are available;
- no external provider is required;
- production remains explicitly `native_only`.

Exit conditions:

- the real permission/path-policy decision creates a versioned immutable grant before job admission;
- the persisted grant survives restart and the executor never fabricates a substitute;
- every nested Broker call verifies grant integrity and full invocation scope;
- notification claim, injection, acknowledgement, suppression, failure, and lease recovery are transactional SQLite operations across independent services;
- restart at every delivery boundary yields exactly one durable parent-session event;
- child lineage is stored in SQLite and queryable by program, parent job, attempt, canonical call ID, and instruction sequence;
- the scheduler recursively cancels and reconciles descendants independently of the parent executor future;
- restart reattaches queued/running children and completed calls without duplicate physical execution;
- checkpoint state is restored and replay validates the complete versioned execution fingerprint;
- replay storage is concurrency-safe, bounded, correctly redacted, and uses correctly labeled SHA-256 integrity values;
- one full-record integrity digest protects the typed result and its real call, child, and output artifacts;
- normal production construction accepts only native execution and never silently falls back from hosted policy;
- process-level daemon kill/restart, failpoint, independent SQLite claimant, capacity-one child, descendant cleanup, and corruption tests pass;
- no unresolved high or medium finding remains;
- an independent reviewer accepts `plans/closure/tool-programs/013-status.md`.

## 4. Dependency graph

```text
M001–M010 implementation and historical closure records
                         |
                         v
M011 production ownership implementation
(conditionally closed after post-closure review)
                         |
                         v
M012 authority/recovery/delivery corrective implementation
(conditionally closed after production-path review)
                         |
                         v
M013 production authority/descendant/recovery closure
                         |
                         v
Strict Tool Programs subsystem closure
```

M013 supersedes M012's strict closure claims only where the post-M012 findings apply. Historical closure records remain traceability artifacts and must not be rewritten to conceal their original evidence.

## 5. Closure authority

Until M013 closes:

- the subsystem status is `active`;
- M011 and M012 are `conditionally closed` and historical, not current strict-closure authorities;
- M002, M005, M007, M008, M009, M010, M011, and M012 remain useful implementation/evidence records but do not independently establish production-boundary closure;
- mutation-capable, destructive, approval-sensitive, shell, patch, Git mutation, commit, push, and subagent tools remain prohibited from the programmable palette;
- production Tool Programs remain `native_only`;
- documentation may not claim scope-verified program authority, transactionally exactly-once notification delivery, scheduler-owned descendant convergence, complete checkpoint/replay recovery, complete artifact integrity, or process-level closure without M013 evidence.

The authoritative implementation and acceptance details are in:

- `plans/implementation/tool-programs/013-production-authority-descendant-and-recovery-closure.md`

The eventual closure record must be created at:

- `plans/closure/tool-programs/013-status.md`

The implementation pass must not create or self-accept that closure record.

## 6. Milestone status

| Milestone | Status | Disposition |
|---|---|---|
| 001 | historical closed | Scheduler-owned ordinary Python foundation retained |
| 002 | historical closed; revalidated by M011–M013 | Canonical Broker foundation retained; M013 owns complete persisted grant verification |
| 003 | historical closed; extended by M013 | Durable domain/storage foundation retained; M013 owns lineage and replay schema depth |
| 004 | historical closed | Restricted-Python frontend and static bounds retained |
| 005 | historical closed; revalidated by M011–M013 | M013 owns checkpoint restoration, full replay binding, and process-level restart proof |
| 006 | historical closed | Read-only programmable palette retained; no authority expansion |
| 007 | historical closed; revalidated by M011–M013 | M013 owns persisted lineage, reattachment, scheduler cancellation, permit convergence, and child artifacts |
| 008 | historical closed; revalidated by M011–M013 | M013 owns transactional delivery and restart proof across independent services |
| 009 | historical closed; capability/library record | Production remains native-only; hosted adapter is not a production Tool Program backend |
| 010 | conditionally closed, historical | Native harness retained; strict closure transferred through M011/M012 to M013 |
| 011 | conditionally closed, historical | Production mechanics retained; post-closure findings transferred to M012/M013 |
| 012 | conditionally closed, historical | Broker failure mapping and native-only truthfulness improved; strict production closure transferred to M013 |
| 013 | ready | Persisted authority, transactional delivery, durable descendants, complete recovery, artifacts, and process-level closure |

## 7. Completion definition

The Tool Programs subsystem becomes strictly closed only when:

1. M013 has an independently accepted closure record;
2. the original roadmap invariants and M013 C-01 through C-45 are true in the production daemon path;
3. authority is derived from, persisted from, and verified against the real permission/path-policy decision;
4. non-success or unauthorized nested calls invoke no underlying tool and cannot enter successful replay state;
5. SQLite is authoritative for notification transitions and delivery identity across independent services and restart;
6. scheduler terminalization cancels and reconciles descendants independently of executor cleanup;
7. process restart restores checkpoint state, reattaches children, and never repeats a durably completed call;
8. source, invocation, grant, attempt, call, child, run, result, artifact, and notification identities are distinct and correctly correlated;
9. foreground, background, notification, and inspection consume one fully integrity-checked typed result;
10. production exposes only runtime-reachable backends;
11. closure-bearing tests exercise real daemon, scheduler, SQLite, artifact, process, and protocol boundaries;
12. full targeted tests, process-level fault tests, migrations, formatting, compilation, and repository-owned static guards pass;
13. broader workspace failures are fixed or documented with evidence that they are unrelated and cannot invalidate M013;
14. live operational and CI evidence is labeled truthfully;
15. roadmap/addendum, implementation plan, closure records, architecture documentation, commit SHAs, and registry agree.
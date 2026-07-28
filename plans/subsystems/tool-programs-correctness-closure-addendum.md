# Tool Programs Correctness and Ownership Closure Addendum

Status: closed — Milestone 015 accepted at independent review

Canonical subsystem roadmap:

- `plans/subsystems/tool-programs-roadmap.md`

Current strict-closure implementation plan:

- `plans/implementation/tool-programs/015-final-production-path-and-independent-closure.md`

Historical predecessors:

- `plans/implementation/tool-programs/014-production-boundary-and-process-evidence-closure.md`
- `plans/closure/tool-programs/014-status.md`
- `plans/implementation/tool-programs/013-production-authority-descendant-and-recovery-closure.md`
- `plans/closure/tool-programs/013-status.md`
- `plans/implementation/tool-programs/012-authority-recovery-and-delivery-corrective-closure.md`
- `plans/closure/tool-programs/012-status.md`
- `plans/implementation/tool-programs/011-production-correctness-and-ownership-closure.md`
- `plans/closure/tool-programs/011-status.md`

M015 baseline reviewed:

- `c9559d23634771dc1bae742da43ae8e362507f6f` (`main`)

Applicable ADR:

- `plans/adrs/ADR-0001-programmatic-tool-execution-authority.md`

## 1. Purpose

This addendum is the active corrective control document for strict native-only Tool Programs closure.

M011 through M014 added substantial production mechanics: durable program identity, canonical Broker routing, typed terminal-outcome handling, scheduler timeout plumbing, call journaling, notification compare-and-set operations, expanded lineage, checkpoint loading, replay fingerprints, typed results, SHA-256 integrity checks, and explicit `native_only` policy.

A post-M014 production-path review found that several strict-closure claims still exceed the implemented mechanisms and evidence. M014 is therefore retained as a historical, conditionally closed implementation record. M015 is the sole active strict-closure authority.

M015 is a narrow final corrective pass. It does not redesign the restricted Python language, broaden the programmable tool palette, or implement hosted Tool Programs.

## 2. Post-M014 trigger findings

The review of `c9559d23634771dc1bae742da43ae8e362507f6f` found:

1. executable authority can still be synthesized from program, workspace, session, or agent-derived fallback values when an accepted permission/path-policy decision is absent;
2. submission and Broker verification use incompatible contract digest algorithms, submission constructs a separate default catalog, and contract-resolution failure can become an empty snapshot;
3. restoring a checkpoint can replace and erase newer completed-call records loaded from the durable ledger;
4. the checkpoint type contains pending-child state, but production checkpoint creation never persists the active child wait before blocking;
5. child artifacts use synthetic job/status digests, omit actual run identity, and large output bypasses the canonical artifact store with manually constructed handles;
6. notification creation still writes memory first, logs durable persistence failures, and can report database recovery failure as zero recovered records;
7. descendant traversal prunes terminal intermediate nodes and therefore can miss active deeper descendants;
8. complete process-group, permit, lease, counter, and capacity convergence remains unproven;
9. the daemon suite does not submit through a public protocol, activate deterministic failpoints, or prove recovery across two independent daemon processes;
10. the implementation pass created its own M014 closure record and claimed independent closure without a separate reviewed commit or mechanism-faithful evidence.

These are unresolved high and medium authorization, contract, replay, child-recovery, notification, artifact, descendant, process-evidence, and governance defects. They invalidate strict M014 closure but do not erase its implementation value.

## 3. Current corrective milestone

### Milestone 015 — Final production-path and independent closure

Class: final corrective implementation / authorization convergence / restart correctness / canonical persistence / process evidence / independent closure

Objective: close the remaining production-path mismatches while retaining the valid M014 work, then establish strict closure through a separate independent review.

Dependencies:

- M001–M014 implementation and historical records are present;
- the normal AgentLoop/Broker permission path, runtime Broker catalog, scheduler, JobStore, SQLite session store, RunStore, artifact store, managed process layer, and native daemon protocol are available;
- no external provider is required;
- production remains explicitly `native_only`;
- M015 does not broaden program authority.

Exit conditions:

- an actual accepted direct-call permission/path-policy decision is required before source persistence or job creation;
- identity-derived values remain correlation-only and cannot create executable authority;
- submission, executor, and Broker verify one frozen runtime contract snapshot with one canonical digest helper;
- a normal authorized nested read-only call succeeds through the production path;
- checkpoint and completed-call recovery merge monotonically and never duplicate a durably completed call;
- active child identity is persisted before waiting and restart reattaches or consumes the same child without duplicate submission;
- call, child, and large-output artifacts use canonical resolvable stores and verified digests;
- notification persistence and session injection fail closed and remain durably idempotent across independent service instances and restart;
- descendant traversal crosses terminal intermediate nodes and scheduler-owned reconciliation converges processes, permits, leases, counters, and capacity;
- real daemon tests submit through a public protocol, activate deterministic failpoints, kill process A, restart process B against the same durable state, and prove exact-once recovery and bounded convergence;
- implementation moves only to `closing`; an independent reviewer later creates and accepts the M015 closure record;
- no unresolved high or medium finding remains.

## 4. Dependency graph

```text
M001–M010 foundations
        |
        v
M011 production ownership implementation
(conditionally closed historical record)
        |
        v
M012 authority/recovery/delivery corrective implementation
(conditionally closed historical record)
        |
        v
M013 production authority/descendant/recovery implementation
(conditionally closed historical record)
        |
        v
M014 production-boundary implementation
(conditionally closed after production-path review)
        |
        v
M015 final production-path and independent closure
        |
        v
Strict native-only Tool Programs subsystem closure
```

Historical records remain traceability artifacts and must not be rewritten to conceal their original claims. Their reconciled closure records identify corrected dispositions and successor criteria.

## 5. Closure authority

Until M015 closes:

- the Tool Programs subsystem status is `active`;
- M011, M012, M013, and M014 are `conditionally closed` historical implementation records;
- M015 is the sole active strict-closure authority;
- mutation-capable, destructive, approval-sensitive, shell, patch, Git mutation, commit, push, and subagent tools remain prohibited from the programmable palette;
- production remains `native_only`;
- documentation may not claim accepted-decision authority, canonical contract convergence, monotonic replay, active-child restart reattachment, exactly-once session delivery, canonical child/output artifacts, full descendant resource convergence, or daemon failpoint closure without M015 evidence.

The authoritative implementation and acceptance details are in:

- `plans/implementation/tool-programs/015-final-production-path-and-independent-closure.md`

The eventual independent closure record must be created at:

- `plans/closure/tool-programs/015-status.md`

The implementation pass must leave that closure record absent.

## 6. Milestone status

| Milestone | Status | Disposition |
|---|---|---|
| 001–009 | historical closed/capability records | Foundations retained; later milestones own production-boundary depth |
| 010 | conditionally closed, historical | Native harness retained; later corrective milestones own strict closure |
| 011 | conditionally closed, historical | Production ownership mechanics landed; strict closure transferred forward |
| 012 | conditionally closed, historical | Broker failure and native-only improvements retained; strict closure transferred forward |
| 013 | conditionally closed, historical | Grant persistence, CAS syntax, basic lineage, replay/result improvements retained; strict closure transferred forward |
| 014 | conditionally closed, historical | Checkpoint loading, v35 lineage, result integrity, native-only enforcement, and other improvements retained; post-review production gaps owned by M015 |
| 015 | closed | Implementation head `247ef50`; independent approval `230f435`; closure `plans/closure/tool-programs/015-status.md` |

## 7. Completion definition

The Tool Programs subsystem becomes strictly closed only when:

1. all M015 C-01 through C-52 criteria are true in production paths;
2. accepted authority derives from the actual direct-call permission/path-policy decision and no executable fallback remains;
3. one frozen runtime contract snapshot is verified consistently from submission through nested Broker execution;
4. restart cannot duplicate a completed call or child submission and cannot extend the original deadline;
5. active child identity is persisted before waiting and recovered through scheduler/JobStore lineage;
6. notification, replay, checkpoint, result, and artifact persistence failures fail closed;
7. foreground, background notification, inspection, and restart consume one integrity-checked typed result with canonical artifact identities;
8. descendant jobs, process groups, permits, leases, counters, and scheduler capacity converge after terminalization or restart;
9. real daemon kill/restart failpoint tests pass through a public protocol boundary without shared in-memory objects;
10. full targeted formatting, compilation, migration, static guard, and repository-standard bounded tests pass;
11. the implementation pass leaves M015 at `closing` and does not create or approve its closure record;
12. a separate reviewer creates `plans/closure/tool-programs/015-status.md` at a later commit with exact implementation head, test evidence, CI/status references when available, and criterion-by-criterion disposition;
13. no unresolved high or medium finding remains;
14. roadmap, addendum, implementation plan, closure record, architecture documentation, and registry agree.

No additional corrective milestone should be registered after M015 unless a new production-path defect is demonstrated.

# Tool Programs Correctness and Ownership Closure Addendum

Status: active — Milestone 014 closing (implementation landed; closure evidence in review)

Canonical subsystem roadmap:

- `plans/subsystems/tool-programs-roadmap.md`

Current strict-closure implementation plan:

- `plans/implementation/tool-programs/014-production-boundary-and-process-evidence-closure.md`

Historical predecessors:

- `plans/implementation/tool-programs/013-production-authority-descendant-and-recovery-closure.md`
- `plans/closure/tool-programs/013-status.md`
- `plans/implementation/tool-programs/012-authority-recovery-and-delivery-corrective-closure.md`
- `plans/closure/tool-programs/012-status.md`
- `plans/implementation/tool-programs/011-production-correctness-and-ownership-closure.md`
- `plans/closure/tool-programs/011-status.md`

Post-M013 baseline reviewed:

- `58e87ff3d82508037ae4912df2ae9b9b8a4ef090` (`main`)

Applicable ADR:

- `plans/adrs/ADR-0001-programmatic-tool-execution-authority.md`

## 1. Purpose

This addendum is the active corrective control document for strict Tool Programs closure.

M011 through M013 added substantial production mechanics: durable program identity, canonical Broker routing, typed terminal-outcome handling, scheduler timeout plumbing, call journaling, notification compare-and-set operations, basic child lineage, expanded replay fingerprints, typed results, and explicit `native_only` policy.

A post-M013 production-path review found that several strict-closure claims still exceed the implemented mechanisms and evidence. M013 is therefore retained as a historical, conditionally closed implementation record. M014 is the sole active strict-closure authority.

M014 is a bounded corrective pass. It does not redesign the restricted Python language, broaden the programmable tool palette, or implement hosted Tool Programs.

## 2. Post-M013 trigger findings

The review of `58e87ff3d82508037ae4912df2ae9b9b8a4ef090` found:

1. Tool Program authority still derives from synthesized program/workspace/session/agent identity material rather than the actual accepted permission and workspace path-policy decision;
2. submission hashes an empty contract summary while Broker verification hashes the concrete invoked contract, so normal authorized nested calls are not proven to work through production admission;
3. the executor loads completed calls but does not load or restore the latest checkpoint;
4. checkpoint state lacks bounded locals/control frames and pending-child state required for safe direct resume, while production replay leaves the original deadline unset;
5. the durable lineage model lacks parent program ID, instruction sequence, and relation kind, uses operation-derived call identity, and common in-memory transitions erase lineage;
6. lineage schema changes were added to an already-applied migration, so existing databases do not receive them;
7. descendant enumeration/cancellation is direct-only rather than recursive and does not establish full process-group, permit, lease, and counter convergence;
8. notification persistence logs and swallows storage errors, recovery still emits MD5 payload identities, and session injection idempotency is not fully schema-enforced through the real session insertion boundary;
9. child artifacts remain without real run/handle/digest identity, and large output bypasses the canonical artifact store through direct filesystem writes and fabricated handles;
10. replay journal concurrency protection is process-local and does not cover overlapping daemon processes or crash/restart boundaries;
11. mandatory daemon start/kill/restart failpoint evidence was explicitly deferred and replaced with in-process store reconstruction;
12. the implementation pass created and accepted its own M013 closure record while marking a mandatory binary criterion deferred.

These are unresolved high and medium authorization, contract, recovery, lineage, notification, artifact, resource, process-evidence, and governance defects. They invalidate strict M013 closure but do not erase the implementation value recorded by M013.

## 3. Current corrective milestone

### Milestone 014 — Production-boundary and process-evidence closure

Class: corrective implementation / authorization / durable recovery / recursive ownership / artifact integrity / process evidence / governance closure

Objective: replace remaining synthetic, partial, process-local, or structurally tested mechanisms with production-boundary implementations and real daemon recovery evidence.

Dependencies:

- M001–M013 implementation and historical records are present;
- the normal AgentLoop/Broker permission path, workspace path policy, scheduler, JobStore, session SQLite store, RunStore, artifact store, managed process layer, and native Tool Program runtime are available;
- no external provider is required;
- production remains explicitly `native_only`;
- M014 does not broaden program authority.

Exit conditions:

- the accepted direct-call permission/path-policy decision produces the immutable persisted grant;
- submission, executor, and Broker use one canonical frozen contract snapshot and digest;
- a normal authorized nested read-only call succeeds through production submission and any contract or policy drift fails before invocation;
- the executor loads and restores complete bounded checkpoint state, including locals/control state, pending child identity, budgets, call sequence, and original absolute deadline;
- replay/checkpoint state is safe across overlapping process lifetimes and daemon restart;
- a new migration upgrades existing databases with complete immutable lineage;
- recursive descendant cancellation, reattachment, process-group cleanup, permit/lease release, and counter convergence are scheduler-owned;
- notification persistence fails closed, uses SHA-256, and parent-session injection is durably idempotent;
- call, child, and output artifacts use canonical resolvable stores and verified digests;
- real daemon process tests submit through a public protocol, activate failpoints, kill the daemon, restart against the same state, and prove bounded convergence;
- implementation moves only to `closing`; an independent reviewer creates and accepts the M014 closure record;
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
(conditionally closed after post-implementation review)
        |
        v
M014 production-boundary and process-evidence closure
        |
        v
Strict Tool Programs subsystem closure
```

Historical records remain traceability artifacts and must not be rewritten to conceal their original claims. The M013 reconciliation record identifies the corrected disposition and successor criteria.

## 5. Closure authority

Until M014 closes:

- the Tool Programs subsystem status is `active`;
- M011, M012, and M013 are `conditionally closed` historical implementation records;
- M014 is the sole active strict-closure authority;
- mutation-capable, destructive, approval-sensitive, shell, patch, Git mutation, commit, push, and subagent tools remain prohibited from the programmable palette;
- production remains `native_only`;
- documentation may not claim real decision-derived authority, canonical contract convergence, complete checkpoint restoration, recursive descendant convergence, exactly-once session delivery, canonical child/output artifacts, cross-process replay safety, or daemon restart closure without M014 evidence.

The authoritative implementation and acceptance details are in:

- `plans/implementation/tool-programs/014-production-boundary-and-process-evidence-closure.md`

The eventual independent closure record must be created at:

- `plans/closure/tool-programs/014-status.md`

## 6. Milestone status

| Milestone | Status | Disposition |
|---|---|---|
| 001–009 | historical closed/capability records | Foundations retained; later milestones own production-boundary depth |
| 010 | conditionally closed, historical | Native harness retained; later corrective milestones own strict closure |
| 011 | conditionally closed, historical | Production ownership mechanics landed; strict closure transferred forward |
| 012 | conditionally closed, historical | Broker failure and native-only improvements retained; strict closure transferred forward |
| 013 | conditionally closed, historical | Grant persistence, CAS syntax, basic lineage, replay/result improvements retained; post-review gaps owned by M014 |
| 014 | ready | Real authority decision, canonical contracts, complete checkpoint recovery, migration-safe lineage, recursive descendants, fail-closed delivery, canonical artifacts, daemon process evidence, and independent closure |

## 7. Completion definition

The Tool Programs subsystem becomes strictly closed only when:

1. M014 has an independently accepted closure record created separately from implementation;
2. all M014 C-01 through C-54 criteria are true in production paths;
3. authority derives from the actual accepted permission/path-policy decision;
4. one canonical contract snapshot is verified consistently from submission through nested Broker execution;
5. checkpoints restore all state required for safe direct resume and retain the original absolute deadline;
6. lineage is complete, immutable, migration-safe, and recursively scheduler-queryable;
7. daemon restart never duplicates durably completed calls, child submissions, or parent-session notifications;
8. notification, replay, checkpoint, result, and artifact persistence failures fail closed;
9. recursive descendant jobs, process groups, permits, leases, and counters converge after terminalization or restart;
10. foreground, background notification, and inspection consume one integrity-checked typed result with canonical artifact identities;
11. real daemon kill/restart failpoint tests pass through the public protocol boundary;
12. full targeted formatting, compilation, migrations, static guards, and repository-standard bounded tests pass;
13. CI/test evidence and commit identities are accurate and independently reviewed;
14. roadmap, addendum, implementation plan, closure record, architecture documentation, and registry agree.
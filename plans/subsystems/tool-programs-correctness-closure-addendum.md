# Tool Programs Correctness and Ownership Closure Addendum

Status: closed — Milestone 011 accepted; live hosted-provider transport remains an operational evidence condition

Canonical subsystem roadmap:

- `plans/subsystems/tool-programs-roadmap.md`

Corrective implementation plan:

- `plans/implementation/tool-programs/011-production-correctness-and-ownership-closure.md`

Post-implementation baseline reviewed:

- `4dbb04e9a402c85ee1dd97d94c55f3951d0debd4` (`main`)

Applicable ADR:

- `plans/adrs/ADR-0001-programmatic-tool-execution-authority.md`

## 1. Purpose

This addendum reopens strict subsystem closure after the implementation of Milestones 001–010 exposed a gap between isolated/library evidence and the production daemon path.

The original roadmap remains the canonical architecture. This addendum owns one corrective milestone that must reconcile production identity, authority, persistence, cancellation, notification, child-job ownership, broker enforcement, hosted-provider selection, and mechanism-faithful closure evidence.

This is a corrective closure milestone, not a redesign and not an expansion of the version-1 programmable tool palette.

## 2. Trigger findings

The post-implementation review found that:

1. logical program identity and submission idempotency are derived primarily from source content rather than one invocation identity;
2. production `tool_program` submission drops parent session, turn, agent, job, attempt, and authority lineage;
3. background notification state is daemon-memory scoped, initial records use an empty session identity, and terminal/restart delivery is not durably proven;
4. the interpreter supports checkpoints and completed-call replay, but the production executor does not durably persist and reload them at call boundaries;
5. the Tool Broker is not yet the sole direct-call execution boundary and does not fully enforce schema, authority, path, cancellation, timeout, artifact, and output contracts;
6. child-job idempotency is configuration-derived rather than call-identity-derived, parent cancellation/deadline lineage is incomplete, and result artifacts are discarded;
7. the configured program timeout is not consistently enforced as the scheduler/executor wall deadline, and interpreter heartbeat does not update durable attempt progress;
8. hosted Responses support is substantial provider infrastructure but is not selected through the normal production runtime;
9. focused fixtures are stronger than the current production restart, parent-notification, and nested-resource evidence;
10. M010's historical conditional closure understates these as operational evidence limitations rather than correctness/ownership findings.

## 3. Corrective milestone

### Milestone 011 — Production correctness and ownership closure

Class: invariant / correctness / recovery / interoperability / final closure

Objective: make the production Tool Program path satisfy the ownership and recovery semantics already stated by the original roadmap, then prove those semantics through public daemon interfaces and process-level fault tests.

Dependencies:

- M001–M010 implementation is present;
- current scheduler, Tool Broker, Tool Program interpreter, provider adapter, notification, projection, artifact, RunStore, and workspace services are available;
- no external service is a hard dependency for native correctness;
- live Eggpool and hosted-provider evidence remain operational inputs for the corresponding interoperability gate.

Exit conditions:

- one logical invocation has one generated durable program identity distinct from source identity;
- all nested calls, child jobs, artifacts, notifications, projections, and provider continuations retain immutable parent lineage;
- all production direct and programmatic calls enter one enforced Tool Broker boundary;
- program attempts persist call reservations, completions, checkpoints, and replay cursors before crossing crash-sensitive boundaries;
- daemon restart never repeats a completed call and detects replay divergence;
- foreground/background timeout, cancellation, heartbeat, and terminal state are scheduler-owned and durable;
- parent notification is session-addressed, terminal-driven, restart-safe, bounded, and exactly-once logically;
- child jobs inherit causal identity, narrowed deadlines, cancellation, resource policy, and artifact/result ownership;
- hosted execution is selectable only through explicit provider capabilities and normal runtime configuration, with no silent semantic fallback;
- mechanism-faithful native, restart, contention, notification, child-job, and hosted-selection tests pass;
- live Eggpool evidence is recorded or remains an explicit operational blocker without weakening native closure;
- no unresolved high or medium correctness, security, recovery, identity, authority, or resource-ownership finding remains.

## 4. Dependency graph

```text
M001–M010 implementation and historical closure records
                         |
                         v
M011 production correctness and ownership closure
                         |
                         v
Strict Tool Programs subsystem closure
```

M011 supersedes earlier strict closure claims only where its audit findings apply. Historical closure records remain for traceability and must not be rewritten to conceal their original evidence. The M011 closure record must explicitly state which earlier findings were accepted, corrected, disproved, or deferred.

## 5. Closure authority

Until M011 closes:

- the subsystem status is `active`;
- M010 remains a historical conditional closure, not the current closure authority;
- M002, M005, M007, M008, and M009 remain useful implementation records but do not independently establish production-boundary closure;
- mutation-capable program tools remain prohibited;
- no documentation may claim restart-safe exactly-once calls, durable exactly-once parent notification, canonical broker ownership, or production hosted execution without M011 evidence.

The authoritative implementation and acceptance details are in:

- `plans/implementation/tool-programs/011-production-correctness-and-ownership-closure.md`

The eventual closure record must be created at:

- `plans/closure/tool-programs/011-status.md`

## 6. Milestone status

| Milestone | Status | Disposition |
|---|---|---|
| 001 | historical closed | Scheduler-owned ordinary Python foundation retained |
| 002 | historical closed; revalidated by M011 | Canonical broker ownership and enforcement require production correction |
| 003 | historical closed | Durable domain/storage foundation retained and extended as needed |
| 004 | historical closed | Restricted-Python frontend and static bounds retained |
| 005 | historical closed; revalidated by M011 | Production checkpoint/replay/heartbeat wiring requires correction |
| 006 | historical closed | Read-only programmable palette retained; no authority expansion |
| 007 | historical closed; revalidated by M011 | Child identity, cancellation, deadline, resource, and artifact ownership require correction |
| 008 | historical closed; revalidated by M011 | Durable terminal notification and parent-session delivery require correction |
| 009 | historical closed; revalidated by M011 | Provider infrastructure retained; production runtime selection requires correction |
| 010 | conditionally closed, historical | Native harness retained; strict closure findings transfer to M011 |
| 011 | closed | Production correctness and ownership closure accepted by `plans/closure/tool-programs/011-status.md` |

## 7. Completion definition

The Tool Programs subsystem becomes strictly closed only when:

1. M011 has an accepted closure record;
2. the original roadmap invariants are true in the production daemon path, not only in component fixtures;
3. process-level restart and cancellation tests prove bounded convergence and no duplicate completed calls;
4. session-addressed notification delivery is durable across claim, injection, acknowledgement, and restart;
5. direct/programmatic/child/hosted execution all use the same authority and Tool Broker semantics;
6. source, invocation, attempt, call, job, run, artifact, and notification identities are distinct and correctly correlated;
7. full targeted tests and repository-owned static guards pass;
8. broader workspace failures are either fixed or documented with evidence that they are unrelated and cannot invalidate this subsystem;
9. live operational evidence is labeled truthfully;
10. roadmap, addendum, implementation plan, closure record, architecture documentation, and registry agree.

---
name: planning
description: CodeGG's plans/ planning process — creating subsystem roadmaps, milestone implementation plans, closure records, and registering/unblocking them in registry.md
version: 1.0.0
tags:
  - planning
  - roadmap
  - registry
  - lifecycle
  - governance
---

# Planning Process Skill

This skill covers the CodeGG planning system under `plans/`. It explains how to create new roadmaps/plans, register them in `plans/registry.md`, execute them, and properly close them out (including deciding whether downstream plans can be unblocked).

CodeGG separates **canonical long-term direction** (stable) from **interim planning** (operational, may evolve with the codebase). Implementation agents MUST NOT silently rewrite the long-term documents.

## When to Load

Load this skill when working on:

- Creating a new subsystem roadmap under `plans/subsystems/`
- Writing a new milestone implementation plan under `plans/implementation/<subsystem>/`
- Writing a closure record under `plans/closure/<subsystem>/`
- Updating `plans/registry.md` (adding dependency-ready plans, marking active/closing/closed, unblocking downstream work)
- Writing an ADR under `plans/adrs/`
- Archiving completed interim documents to `plans/archive/`
- Reviewing whether a finished milestone unblocks other registered plans
- Trying to understand the status vocabulary (`proposed`, `ready`, `active`, `blocked`, `closing`, `closed`, `conditionally closed`, `superseded`, `archived`)

## Document Hierarchy

```text
Long-term specification and terminology      (canonical, stable)
        |
        v
Architecture decision records (adrs/)        (durable decisions)
        |
        v
Master long-term roadmap (002-)              (canonical sequencing)
        |
        v
Subsystem roadmaps (subsystems/)             (coherent workstreams)
        |
        v
Milestone implementation plans               (bounded agent handoff)
        |
        v
Implementation and verification
        |
        v
Closure records (closure/)                   (gate that determines completion)
        |
        v
Archive (archive/)                           (historical traceability)
```

Interim plans MUST reference canonical documents rather than duplicating them. Repository evidence overrides interim plans, but long-term invariants override both.

## Document Classes

| Class | Location | Stability | Purpose |
|---|---|---|---|
| Canonical long-term | `plans/000-`, `plans/001-`, `plans/002-`, `plans/003-` | Very stable; requires ADR or user direction to amend | What CodeGG is becoming and what must remain true |
| ADR | `plans/adrs/ADR-NNNN-*.md` | Immutable once accepted; supersede, don't rewrite | One durable architectural decision across milestones |
| Subsystem roadmap | `plans/subsystems/<name>-roadmap.md` | Stable per workstream; may evolve with evidence | Translates long-term requirements into ordered milestones |
| Milestone implementation plan | `plans/implementation/<subsystem>/NNN-*.md` | Operational; may be corrected | Bounded handoff for one implementation agent |
| Closure record | `plans/closure/<subsystem>/NNN-status.md` | Immutable except factual corrections | Evidence-based gate that determines completion |
| Archive | `plans/archive/` | Historical only | Completed, superseded, or abandoned interim docs |

## Authoritative Order (when documents conflict)

1. Canonical long-term specification and terminology
2. Accepted ADRs
3. Subsystem roadmap
4. Milestone implementation plan
5. Current repository evidence

When repository evidence conflicts with the plan, preserve long-term invariants, record the discrepancy, and make the smallest coherent adjustment. Never invent a new architecture to finish the checklist.

## Status Vocabulary (registry.md)

| Status | Meaning |
|---|---|
| `proposed` | Exists but not approved for execution |
| `ready` | Dependencies satisfied; plan may be handed off |
| `active` | Implementation in progress |
| `blocked` | Named dependency or evidence requirement prevents progress |
| `closing` | Implementation landed; closure evidence being gathered |
| `closed` | Closure record accepted |
| `conditionally closed` | Substantial work landed but named correctness finding prevents strict closure |
| `superseded` | Replaced by another document |
| `archived` | No longer active; retained for traceability |

## Naming Conventions

- ADR: `adrs/ADR-NNNN-short-title.md` (monotonically increasing; never reused)
- Subsystem roadmap: `subsystems/<subsystem>-roadmap.md` (no dates in filenames)
- Implementation plan: `implementation/<subsystem>/NNN-short-title.md` (number local to subsystem)
- Closure record: `closure/<subsystem>/NNN-status.md` (same number as plan)
- Archive: preserve original relative structure under `archive/`

## Work Classification

Every planned item MUST be assigned one primary class:

- **Invariant** — must remain true across releases (e.g., singleton daemon, immutable in-flight snapshots, stable project identity). Requires static guards, property tests, or architecture-level evidence.
- **Capability** — user-, developer-, operator-, or integration-visible behavior. Requires end-to-end acceptance evidence.
- **Infrastructure** — internal machinery used by capabilities (e.g., durable asset registry, presence lease store). MUST NOT be represented as completed capability until a consumer path exists.
- **Polish** — ergonomics, diagnostics, performance, cleanup, docs. Should follow functional and correctness closure.

Infrastructure and polish MUST NOT be presented as completed user capability unless user-visible acceptance criteria are satisfied.

## Lifecycle

1. Identify relevant canonical long-term sections.
2. Record any unresolved architectural decision in `adrs/` (new ADR).
3. Create or update a subsystem roadmap in `subsystems/`.
4. Select one dependency-ready milestone.
5. Write a bounded handoff plan under `implementation/<subsystem>/`.
6. Register the plan in `plans/registry.md` (move from `ready` to `active` when work begins).
7. Implement and verify.
8. Write a closure record under `closure/<subsystem>/`.
9. Update `registry.md` and the subsystem roadmap status.
10. Archive completed interim documents when they no longer represent active work.
11. Audit blocked work: if any registered plan's blocker is resolved by this closure, register/register it as `ready` in the same commit.

A milestone is complete only when closure evidence defined by its implementation plan and subsystem roadmap is satisfied. A commit message saying "closed" is not closure evidence.

## Creating a New Subsystem Roadmap

Path: `plans/subsystems/<subsystem>-roadmap.md`

Required structure (see `plans/subsystems/README.md` for full template):

```markdown
# <Subsystem> Roadmap

Status: proposed | active | closing | closed | superseded

Long-term references:
- `plans/000-long-term-specification.md#...`
- `plans/001-terminology-and-domain-model.md#...`
- `plans/002-long-term-roadmap.md#...`

Related ADRs:
- `plans/adrs/ADR-NNNN-...md` (or "None required")

## 1. Purpose and ownership boundary
## 2. Work classification (invariants / capabilities / infrastructure / polish)
## 3. Non-goals
## 4. Current state (repo evidence, no fragile line numbers)
## 5. Target architecture
## 6. Dependency graph (with hard/interface/soft/operational classification)
## 7. Milestones (each with class, objective, dependencies, deliverable boundary, exit conditions)
## 8. Cross-cutting requirements (storage, protocol, security, concurrency, observability, docs)
## 9. Verification strategy
## 10. Risks and decision points
## 11. Completion definition
## 12. Milestone status table
```

Rules:

- MUST link to canonical long-term requirements rather than duplicating them.
- MUST distinguish infrastructure from completed capability.
- MUST expose dependencies and decision points.
- SHOULD avoid commit-specific file lists and exact line numbers.
- MUST state non-goals to prevent scope expansion.
- Create only roadmaps ready to be reasoned about. Do not generate all candidate files merely to populate the directory.

After creating, add a row to `plans/registry.md` Active subsystem roadmaps section with `Status: proposed` (or `active`).

## Writing an Implementation Plan

Path: `plans/implementation/<subsystem>/NNN-short-title.md`

Required structure (see `plans/implementation/README.md`):

```markdown
# <Subsystem> Milestone NNN — <Title>

Status: ready for handoff | active | blocked | implemented | superseded
Repository baseline: `<commit SHA or branch state>`

Source roadmap: `plans/subsystems/<subsystem>-roadmap.md#...`
Long-term requirements: (links)
Applicable ADRs: (links)
Primary class: invariant | capability | infrastructure | polish

## 1. Objective (one bounded outcome)
## 2. Why this milestone is ready (closed hard deps, stable interface deps)
## 3. Current implementation evidence
## 4. Invariants that must not regress
## 5. Scope (in / out)
## 6. Required production changes (core, storage, protocol, runtime, frontend, security, docs)
## 7. Ordered work packages (each with intent, required changes, acceptance evidence)
## 8. Failure, cancellation, restart, contention semantics
## 9. Compatibility and migration
## 10. Required tests (focused, integration, restart, contention, security, migration)
## 11. Required verification commands (exact commands)
## 12. Documentation updates
## 13. Acceptance criteria (externally observable)
## 14. Stop conditions (when agent must report rather than improvise)
## 15. Closure evidence required
## 16. Handoff notes
```

Rules:

- MUST be independently executable, bounded, and tied to a repository baseline.
- The agent may adjust file-level mechanics based on current code, but MUST NOT weaken canonical invariants or silently enlarge scope.
- Sizing: small enough for one coherent pass (one ownership boundary, production changes, tests, verification, docs). Too large if it combines several releasable capability boundaries, unrelated migrations, or several unresolved ADRs. Too small if it only renames symbols unless required to unblock another milestone.
- Prefer vertical slices (one complete contract) over broad horizontal refactors.

## Writing an ADR

Path: `plans/adrs/ADR-NNNN-short-title.md`

Use an ADR when a decision cannot be answered safely inside one implementation plan without establishing a reusable architectural contract. Required for decisions that:

- Change a canonical identity or ownership boundary
- Introduce a new daemon/frontend/provider/storage protocol
- Select a durable external standard or dependency
- Change authentication or authorization semantics
- Change scheduler or execution authority
- Change consistency, replication, recovery, or fencing semantics
- Establish a public compatibility contract
- Materially change a long-term non-goal

NOT required for: local refactors, internal naming, implementation-specific data structures, reversible optimizations preserving established contracts.

Required sections: Status, Date, Decision owners, Related specification sections, Affected subsystem roadmaps, Context, Decision drivers, Considered options (with benefits/costs/failure modes), Decision, Consequences (positive/negative/neutral), Compatibility and migration, Security and reliability implications, Verification, Supersession.

Status lifecycle: `proposed -> accepted -> deprecated or superseded` (or `rejected`). Accepted ADRs are immutable historical records; supersede, don't rewrite.

## Writing a Closure Record

Path: `plans/closure/<subsystem>/NNN-status.md` (same number as source plan)

Required structure (see `plans/closure/README.md`):

```markdown
# <Subsystem> Milestone NNN — Closure Status

Status: closed | conditionally closed | corrective pass required | blocked
Source implementation plan: `plans/implementation/<subsystem>/NNN-...md`
Source subsystem roadmap: `plans/subsystems/<subsystem>-roadmap.md#...`
Repository baseline reviewed: `<SHA>`
Implementation commits: `<SHA> — summary`

## 1. Executive finding
## 2. Requirement-to-evidence matrix
## 3. Production implementation evidence
## 4. Verification executed (commands + results; label local vs CI truthfully)
## 5. Invariant review
## 6. Failure and recovery review
## 7. Migration and compatibility review
## 8. Security review
## 9. Documentation and operations
## 10. Unresolved findings (severity: critical/high/medium/low)
## 11. Roadmap disposition
## 12. Registry updates
```

A milestone MUST NOT be marked `closed` when:

- Only compilation or formatting was verified
- Required tests were not run and no justified substitute exists
- A user-visible capability has only internal infrastructure
- A security or migration requirement is unimplemented
- A daemon-owned path still bypasses the required authority
- A known high-severity defect remains
- Closure depends on unrecorded assumptions

A milestone MAY be `conditionally closed` when production is complete but named external/operational evidence cannot be obtained in the current environment. The condition, risk, and exact future evidence MUST be explicit.

## Corrective Passes

A corrective pass is a new implementation plan, not an amendment pretending the original succeeded. Path: same subsystem directory, new number (e.g., `004-correctness-and-recovery-closure.md`).

Corrective plans MUST:

- Reference the original plan and closure record.
- List each unclosed requirement or discovered defect.
- Explain why original verification did not catch it.
- Include regression tests or guards preventing recurrence.
- Avoid reopening already-closed scope without evidence.

Repeated corrective passes indicate the subsystem roadmap or milestone sizing needs revision.

## Registry.md Maintenance Rules

`plans/registry.md` is the compact control surface. It links active documents and blockers without duplicating their detailed requirements. Required contents:

- Active subsystem roadmaps (with current milestone and dependencies/blockers)
- Dependency-ready implementation plans
- Active closure work
- Blocked work and blockers
- Recently closed or conditionally closed work (with commit references)
- Deferred unregistered product work

Maintenance rules:

1. Add a subsystem roadmap when it becomes active, not merely because it is a possible future track.
2. Register an implementation plan as `ready` only after dependency and handoff review.
3. Move a plan from `ready` to `active` when implementation begins.
4. Move it to `closing` when production work lands and closure review starts.
5. Mark it `closed` only when the linked closure record says closed AND no unresolved high/medium finding remains.
6. Use `conditionally closed` when a post-closure correctness finding invalidates a strict claim.
7. Record blockers precisely and link the document that owns their resolution.
8. Remove closed rows from active sections after recording them under recently closed work.
9. Periodically archive old closed interim documents while preserving links.
10. Do not copy detailed milestone requirements into this registry.
11. **When one milestone closes, audit blocked work: if any registered plan's blocker is now satisfied, register/register it as `ready` in the same commit.** This is the unblock check.

## Unblocking Downstream Work (Critical Step)

When closing a milestone (writing the closure record), you MUST audit the registry's **Blocked work** section and the dependency graphs in affected subsystem roadmaps:

1. Identify every registered implementation plan whose dependency graph lists the just-closed milestone as a hard or interface dependency.
2. For each such plan, check whether ALL other hard dependencies are now closed AND interface dependencies have stable contracts.
3. If yes, move that plan from `blocked` (or `proposed`) to `ready` in `plans/registry.md`, update its status line, and note the resolution in the closure record's Registry updates section.
4. If other dependencies remain, leave it `blocked` and update the blocker description if the blocker is now partial.
5. Also check whether the closure created new follow-up work (corrective pass required, deferred product work). Register any corrective plan under the same subsystem immediately so it isn't lost.
6. Never silently unblock plans without recording the dependency audit in the closure record.

Example: closing M012 of session-projections unblocked nothing because no registered future plan listed M012 as a hard or interface dependency. Deferred product work (cross-tab artifact hand-off, presence/chat, etc.) remained intentionally unregistered because it is not dependency-ready correctness work.

## Archive Workflow

When moving documents to `plans/archive/`:

1. Preserve original planning category and subsystem grouping (`archive/subsystems/`, `archive/implementation/<subsystem>/`, `archive/closure/<subsystem>/`).
2. Update inbound links from `plans/registry.md` and current subsystem roadmaps.
3. Add a short archival note stating final status and replacement.
4. Preserve Git history through a move (`git mv`) rather than recreating content.
5. Do not rewrite historical conclusions to match later implementation.
6. Ensure active documents link to the replacement or later milestone.

What does NOT belong in archive: canonical long-term documents, accepted ADRs, active subsystem roadmaps, ready/active implementation plans, unresolved closure records.

## Required Closure Review Before Handoff

Before assigning an implementation plan to an agent, verify:

1. Correct long-term references
2. Unresolved architecture decisions have ADRs or are explicitly out of scope
3. Dependency readiness (all hard deps closed, interface deps have stable contracts)
4. Bounded scope and explicit non-goals
5. Explicit ownership and invariants
6. Migration and compatibility effects
7. Concurrency, cancellation, restart, failure semantics
8. Security and authorization effects
9. Required test and static-guard evidence
10. Unambiguous closure criteria

If these are not answerable, the work is not ready for implementation handoff.

## Planning Anti-Patterns (prohibited)

- Adding transient TODO checklists to canonical long-term specification
- One roadmap mixing all subsystems at file granularity
- Handing an agent a broad product goal without a bounded milestone contract
- Equating compilation with closure
- Changing terminology independently in each subsystem
- Implementation plans silently overriding accepted architecture
- Retaining stale active plans after the repository materially changed
- Repeating requirements across files without one authoritative source
- Recording only successful evidence while omitting blocked or unrun verification
- Creating polish phases before the capability's correctness boundary is closed

## Quick Templates

### New subsystem roadmap (header only — full template in `plans/subsystems/README.md`)

```markdown
# <Subsystem> Roadmap

Status: proposed

Long-term references:
- `plans/000-long-term-specification.md#...`
- `plans/001-terminology-and-domain-model.md#...`
- `plans/002-long-term-roadmap.md#phase-N--...`

Related ADRs:
- None required initially.

## 1. Purpose and ownership boundary
## 2. Work classification
### Invariants
### Capabilities
### Infrastructure
### Polish
## 3. Non-goals
## 4. Current state
## 5. Target architecture
## 6. Dependency graph
## 7. Milestones (M1, M2, ...)
## 8. Cross-cutting requirements
## 9. Verification strategy
## 10. Risks and decision points
## 11. Completion definition
## 12. Milestone status table

| Milestone | Status | Implementation plan | Closure record | Blockers |
|---|---|---|---|---|
| 1 | not started | — | — | — |
```

### Registry subsystem row (status transitions)

When adding to `plans/registry.md`:

```markdown
| Subsystem | Status | Roadmap | Current milestone | Dependencies or blockers |
|---|---|---|---|---|
| <name> | proposed | `plans/subsystems/<name>-roadmap.md` | — | — |
| <name> | active | `plans/subsystems/<name>-roadmap.md` | Milestone NNN active | none |
| <name> | closing | `plans/subsystems/<name>-roadmap.md` | Milestone NNN closure review | closure at `plans/closure/<name>/NNN-status.md` |
| <name> | closed | `plans/subsystems/<name>-roadmap.md` | Milestone NNN closed | — |
```

### Dependency-ready implementation plan entry

```markdown
## Dependency-ready implementation plans

- `<Subsystem> Milestone NNN` — `<title>` at `plans/implementation/<subsystem>/NNN-*.md`. Depends on: Milestones X, Y closed; interface contract Z stable. Class: capability.
```

## Related Files

| Path | Role |
|---|---|
| `plans/README.md` | Top-level planning system guide |
| `plans/000-long-term-specification.md` | Canonical end-state specification |
| `plans/001-terminology-and-domain-model.md` | Canonical language and identity model |
| `plans/002-long-term-roadmap.md` | Dependency-ordered long-term capability roadmap |
| `plans/003-planning-process.md` | Normative planning governance (canonical) |
| `plans/registry.md` | Active planning control surface |
| `plans/adrs/README.md` | ADR template and threshold rules |
| `plans/subsystems/README.md` | Subsystem roadmap template |
| `plans/implementation/README.md` | Implementation plan template |
| `plans/closure/README.md` | Closure record template |
| `plans/archive/README.md` | Archive workflow |

## Operational Notes

- **Authority order when conflicts arise**: canonical long-term > accepted ADRs > subsystem roadmap > implementation plan > current repository evidence.
- **One commit = one status change** in registry.md whenever a plan moves between `ready`/`active`/`closing`/`closed`/`blocked`.
- **Closure commit is not the close commit**: the closure record (`closure/<subsystem>/NNN-status.md`) is the gate. The plan itself is not "closed" until that record exists and is accepted.
- **Plan number is local to subsystem**: M001 in session-projections is unrelated to M001 in any other subsystem.
- **Subsystem names are stable**: use the same `kebab-case` name across roadmap, implementation, closure, and registry entries. Do not encode dates.
- **Status labels in registry must match the plan/roadmap/closure record** — contradictions are a closure defect (see M011–M012 history).
# CodeGG Active Planning Registry

This file is the compact control surface for interim planning. It links active documents and blockers without duplicating their detailed requirements.

Canonical direction remains in:

- `plans/000-long-term-specification.md`
- `plans/001-terminology-and-domain-model.md`
- `plans/002-long-term-roadmap.md`
- `plans/003-planning-process.md`

## Status vocabulary

- **proposed** — roadmap or plan exists but is not approved for execution.
- **ready** — dependencies and interfaces are satisfied; plan may be handed off.
- **active** — implementation or closure work is in progress.
- **blocked** — a named dependency or evidence requirement prevents progress.
- **closing** — implementation landed and closure evidence is being gathered.
- **closed** — closure record accepted.
- **superseded** — replaced by another document.
- **archived** — no longer active and retained for traceability.

## Active subsystem roadmaps

| Subsystem | Status | Roadmap | Current milestone | Dependencies or blockers |
|---|---|---|---|---|
| Domain identity and compatibility | active | `plans/subsystems/domain-identity-roadmap.md` | Milestone 1 — typed identity primitives and relation contracts | None |
| Runtime assets and harness interoperability | active | `plans/subsystems/runtime-assets-roadmap.md` | Milestone 1 — source-aware registry and portable skill discovery | Domain identity authoritative project/workspace context |
| Provider connections and Eggpool | active | `plans/subsystems/provider-connections-roadmap.md` | Milestone 1 — durable connection and secret-reference foundation | Domain identity typed IDs |
| Project catalog and lazy discovery | active | `plans/subsystems/project-catalog-roadmap.md` | Milestone 1 — durable project and repository catalog | Domain identity storage and relations |
| Multi-project TUI and sessions | active | `plans/subsystems/tui-project-sessions-roadmap.md` | Milestone 1 — project-aware state and catalog client | Runtime asset refresh and project catalog protocol |
| Frontend-neutral session projections | active | `plans/subsystems/session-projections-roadmap.md` | Milestone 1 — projection contracts and canonical reducer | Domain identity, project catalog, and multi-project TUI state |

## Dependency-ready implementation plans

| Subsystem | Milestone | Class | Plan | Baseline | Status |
|---|---|---|---|---|---|
| Domain identity and compatibility | 001 — typed identity foundation | invariant | `plans/implementation/domain-identity/001-typed-identity-foundation.md` | `fbae374a2cd6172505204b1bc1bee1ef247afd5f` production code | ready |

## Active closure work

| Subsystem | Milestone | Closure record | Status | Open findings |
|---|---|---|---|---|
| — | — | — | — | None. |

## Blocked work

| Work item | Blocker | Required resolution | Owner document |
|---|---|---|---|
| Runtime Assets 001 — project asset registry | Stable authoritative project/workspace identity interface is not closed | Close Domain Identity milestones needed by the roadmap; do not substitute `PWD` or path-derived project IDs | `plans/implementation/runtime-assets/001-project-asset-registry.md` |
| Provider Connections 001 — durable connection foundation | `ProviderConnectionId` and project scope types are not closed | Close Domain Identity Milestone 001 | `plans/implementation/provider-connections/001-connection-foundation.md` |
| Project Catalog 001 — durable catalog foundation | Durable project/repository/workspace/session relations are not closed | Close Domain Identity storage/migration milestone after Milestone 001 | `plans/implementation/project-catalog/001-durable-catalog-foundation.md` |
| Multi-Project TUI 001 — project-aware state | Catalog protocol and runtime asset refresh/generation interfaces are unavailable | Close required Project Catalog and Runtime Assets milestones | `plans/implementation/tui-project-sessions/001-project-aware-state.md` |
| Session Projections 001 — projection contracts | Stable project/session routing and project-aware TUI state are unavailable | Close Domain Identity daemon/protocol adoption, Project Catalog protocol migration, and Multi-Project TUI state foundation | `plans/implementation/session-projections/001-projection-contracts.md` |

## Recently closed work

| Subsystem | Milestone | Closure record | Closed at commit | Follow-up |
|---|---|---|---|---|
| — | — | — | — | None. |

## Registry maintenance rules

1. Add a subsystem roadmap when it becomes active, not merely because it is a possible future track.
2. Register an implementation plan as dependency-ready only after dependency and handoff review.
3. Move a plan from ready to active when implementation begins.
4. Move it to closing when production work lands and closure review starts.
5. Mark it closed only when the linked closure record says closed.
6. Record blockers precisely and link the document that owns their resolution.
7. Remove closed rows from active sections after recording them under recently closed work.
8. Periodically move old closed interim documents to `plans/archive/` while preserving links.
9. Do not copy detailed milestone requirements into this registry.
10. When one milestone closes, update the subsystem roadmap and create/register only the next dependency-ready handoff plan.

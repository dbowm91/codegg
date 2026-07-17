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
| Domain identity and compatibility | active | `plans/subsystems/domain-identity-roadmap.md` | Milestone 3 — daemon and protocol adoption | Milestone 2 closed; next implementation plan not yet authored |
| Runtime assets and harness interoperability | active | `plans/subsystems/runtime-assets-roadmap.md` | Milestone 1 — source-aware registry and portable skill discovery | Plan is dependency-ready after Domain Identity Milestone 2 |
| Provider connections and Eggpool | active | `plans/subsystems/provider-connections-roadmap.md` | Milestone 4 — lifecycle, rotation, refresh, and disable/delete UX | Milestone 3 closed; Milestone 4 ready for handoff |
| Project catalog and lazy discovery | active | `plans/subsystems/project-catalog-roadmap.md` | Milestone 1 closed; Milestone 2 — bounded discovery and reconciliation | Plan is dependency-ready after Domain Identity Milestone 2 |
| Multi-project TUI and sessions | active | `plans/subsystems/tui-project-sessions-roadmap.md` | Milestone 1 — project-aware state and catalog client | Project catalog protocol (catalog service now available; protocol surface arrives with Project Catalog 4) |
| Frontend-neutral session projections | active | `plans/subsystems/session-projections-roadmap.md` | Milestone 1 — projection contracts and canonical reducer | Domain identity (closed), project catalog (Milestone 1 closed), and multi-project TUI state |

## Dependency-ready implementation plans

| Subsystem | Milestone | Class | Plan | Baseline | Status |
|---|---|---|---|---|---|
| Runtime assets and harness interoperability | 001 — project asset registry | infrastructure | `plans/implementation/runtime-assets/001-project-asset-registry.md` | `fbae374` | ready |

## Active closure work

| Subsystem | Milestone | Closure record | Status | Open findings |
|---|---|---|---|---|
| — | — | — | — | None. |

## Blocked work

| Work item | Blocker | Required resolution | Owner document |
|---|---|---|---|
| Multi-Project TUI 001 — project-aware state | Project catalog protocol surface and runtime asset refresh/generation interfaces are unavailable | Close Project Catalog 4 (protocol/server migration) and the Runtime Assets activation/refresh milestone | `plans/implementation/tui-project-sessions/001-project-aware-state.md` |
| Session Projections 001 — projection contracts | Stable project/session routing and project-aware TUI state are unavailable | Close Domain Identity daemon/protocol adoption, Project Catalog 4 (protocol/server migration), and Multi-Project TUI 1 (state foundation) | `plans/implementation/session-projections/001-projection-contracts.md` |

## Recently closed work

| Subsystem | Milestone | Closure record | Closed at commit | Follow-up |
|---|---|---|---|---|
| Project catalog and lazy discovery | 001 — durable catalog foundation | `plans/closure/project-catalog/001-status.md` | `a2db5e4` | Milestone 2 — bounded discovery and reconciliation |
| Provider connections and Eggpool | 003 — session/model selection by connection | `plans/closure/provider-connections/003-status.md` | `213783e` | Milestone 4 — lifecycle, rotation, refresh, and disable/delete UX |
| Domain identity and compatibility | 002 — project/repository storage migration | `plans/closure/domain-identity/002-status.md` | `84d92f0` | Runtime Assets 001 and Project Catalog 001 are ready |
| Domain identity and compatibility | 001 — typed identity foundation | `plans/closure/domain-identity/001-status.md` | `f203ed9` | Milestone 2 — project/repository storage migration |
| Provider connections and Eggpool | 002 — Eggpool `/connect` workflow | `plans/closure/provider-connections/002-status.md` | `8c1675c` | Milestone 3 — session/model selection by connection |
| Provider connections and Eggpool | 001 — durable connection foundation | `plans/closure/provider-connections/001-status.md` | `bccca00` | Milestone 2 — Eggpool connect workflow |

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

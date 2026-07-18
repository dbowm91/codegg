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
| Domain identity and compatibility | active | `plans/subsystems/domain-identity-roadmap.md` | Milestone 4 — closure and legacy-removal criteria | Milestone 3 corrective implementation is complete; the next removal-criteria handoff is not yet registered |
| Runtime assets and harness interoperability | active | `plans/subsystems/runtime-assets-roadmap.md` | Milestone 3 — refresh lifecycle and operator surface | Milestone 2 closed; Milestone 3 handoff plan is dependency-ready |
| Provider connections and Eggpool | active | `plans/subsystems/provider-connections-roadmap.md` | Milestone 5 — corrective lifecycle, rotation, health, and closure | Milestone 4 corrective pass required; Milestone 5 registered as the corrective implementation plan |
| Project catalog and lazy discovery | active | `plans/subsystems/project-catalog-roadmap.md` | Milestone 3 — lazy activation and health | Milestone 2 closed; blocked on the Runtime Assets Milestone 3 refresh/activation interface |
| Multi-project TUI and sessions | active | `plans/subsystems/tui-project-sessions-roadmap.md` | Milestone 1 — project-aware state and catalog client | Project Catalog 4 protocol/server migration and Runtime Assets refresh/activation interfaces |
| Frontend-neutral session projections | active | `plans/subsystems/session-projections-roadmap.md` | Milestone 1 — projection contracts and canonical reducer | Domain Identity 3, Project Catalog 4, and Multi-Project TUI 1 |

## Dependency-ready implementation plans

| Subsystem | Milestone | Class | Plan | Baseline | Status |
|---|---|---|---|---|---|
| Provider connections and Eggpool | 004 — lifecycle, rotation, health, and closure | capability | `plans/implementation/provider-connections/004-lifecycle-rotation-health-closure.md` | `3ce0a7e` | corrective pass required (see active closure work) |
| Provider connections and Eggpool | 005 — corrective lifecycle, rotation, health, and closure | capability | `plans/implementation/provider-connections/005-corrective-lifecycle-rotation.md` | `213272c` | ready |
| Runtime assets and harness interoperability | 003 — refresh lifecycle and operator surface | capability | `plans/implementation/runtime-assets/003-refresh-lifecycle-operator-surface.md` | `5974976` | ready |

## Active closure work

| Subsystem | Milestone | Closure record | Status | Open findings |
|---|---|---|---|---|
| Provider connections and Eggpool | 004 — lifecycle, rotation, health, and closure | `plans/closure/provider-connections/004-status.md` | corrective pass required | Rotation, refresh coordinator, lifecycle states, session/protocol reconciliation, TUI lifecycle controls, fake-daemon harness, and the flaky provisioning test are unfulfilled; corrective plan filed as Milestone 5. |

## Blocked work

| Work item | Blocker | Required resolution | Owner document |
|---|---|---|---|
| Multi-Project TUI 001 — project-aware state | Project catalog protocol/server surface and runtime asset refresh/generation interfaces are unavailable | Close Project Catalog 004 and the Runtime Assets refresh/activation milestones; do not create frontend-local project authority | `plans/implementation/tui-project-sessions/001-project-aware-state.md` |
| Session Projections 001 — projection contracts | Project Catalog 004 and project-aware TUI state are unavailable | Close Project Catalog 004 and Multi-Project TUI 001; consume the now-closed Domain Identity 003 identity contract | `plans/implementation/session-projections/001-projection-contracts.md` |

## Recently closed work

| Subsystem | Milestone | Closure record | Closed at commit | Follow-up |
|---|---|---|---|---|
| Domain identity and compatibility | 003 — corrective daemon and protocol adoption | `plans/closure/domain-identity/003-corrective-status.md` | `ec42dce` | Milestone 4 — closure and legacy-removal criteria |
| Runtime assets and harness interoperability | 002 — explicit-context agent and instruction resolution | `plans/closure/runtime-assets/002-status.md` | `155f7f3` (Milestone 2) | `plans/implementation/runtime-assets/003-refresh-lifecycle-operator-surface.md` |
| Runtime assets and harness interoperability | 001 — project asset registry | `plans/closure/runtime-assets/001-status.md` | `f9db5c3` | Milestone 2 — explicit-context agent and instruction resolution |
| Project catalog and lazy discovery | 002 — bounded discovery and reconciliation | `plans/closure/project-catalog/002-status.md` | `5974976` (implementation) | Milestone 3 — lazy activation and health; blocked on Runtime Assets Milestone 3 |
| Project catalog and lazy discovery | 001 — durable catalog foundation | `plans/closure/project-catalog/001-status.md` | `a2db5e4` | Milestone 2 — bounded discovery and reconciliation |
| Provider connections and Eggpool | 003 — session/model selection by connection | `plans/closure/provider-connections/003-status.md` | `efe1995` | Milestone 5 — corrective lifecycle, rotation, health, and closure (Milestone 4 is corrective-pass-required) |
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

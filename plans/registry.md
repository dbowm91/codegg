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
| Domain identity and compatibility | closed | `plans/subsystems/domain-identity-roadmap.md` | Milestone 4 — closure and legacy-removal criteria (closed) | — |
| Runtime assets and harness interoperability | closed | `plans/subsystems/runtime-assets-roadmap.md` | Milestone 4 — immutable runtime pinning and closure (closed) | — |
| Provider connections and Eggpool | closed | `plans/subsystems/provider-connections-roadmap.md` | Milestone 5 — corrective lifecycle, rotation, health, and closure (closed) | — |
| Project catalog and lazy discovery | closed | `plans/subsystems/project-catalog-roadmap.md` | Milestone 4 — protocol, server migration, and closure (closed) | — |
| Multi-project TUI and sessions | active | `plans/subsystems/tui-project-sessions-roadmap.md` | Milestone 3 — project-correct event routing and lifecycle | Milestone 2 closed at `f569386`; M3 plan registered and ready |
| Frontend-neutral session projections | active | `plans/subsystems/session-projections-roadmap.md` | Milestone 2 corrective — daemon integration and strict closure | Library layer landed at `8dc4b85`; corrective plan registered for canonical publication, dispatch, routing, receiver ownership, and binding-revision closure |

## Dependency-ready implementation plans

| Subsystem | Milestone | Class | Plan | Baseline | Status |
|---|---|---|---|---|---|
| Multi-project TUI and sessions | 003 — project-correct event routing and lifecycle | correctness / lifecycle | `plans/implementation/tui-project-sessions/003-project-correct-event-routing-lifecycle.md` | `f569386` | ready |
| Frontend-neutral session projections | 002 corrective — daemon integration and closure | correctness / integration | `plans/implementation/session-projections/002-corrective-daemon-integration-and-closure.md` | `8c23269` | ready |

## Active closure work

| Subsystem | Milestone | Closure record | Status | Open findings |
|---|---|---|---|---|
| Frontend-neutral session projections | 002 — scoped subscriptions and durable replay | `plans/closure/session-projections/002-status.md` | conditionally closed at `8dc4b85` (library layer) | Central daemon publication; canonical non-empty context; real stream IDs; request dispatch; subscription-receiver ownership; client-isolated live routing; binding-revision invalidation; startup/maintenance integration |

## Authored blocked implementation plans

| Subsystem | Milestone | Plan | Blocker / activation criterion |
|---|---|---|---|
| Multi-project TUI and sessions | 004 — persistent restoration, resource bounds, and closure | `plans/implementation/tui-project-sessions/004-persistent-restoration-resource-closure.md` | Strict TUI Milestone 003 closure |
| Frontend-neutral session projections | 003 — visibility, redaction, and artifact handles | `plans/implementation/session-projections/003-visibility-redaction-artifact-handles.md` | Strict Projection M2 corrective closure plus a transport-derived principal/capability filtering seam |
| Frontend-neutral session projections | 004 — frontend adoption, compatibility, and closure | `plans/implementation/session-projections/004-frontend-adoption-compatibility-closure.md` | Projection M3 closure plus Multi-Project TUI M3 routing/lifecycle interfaces |

## Blocked work

| Work item | Blocker | Required resolution | Owner document |
|---|---|---|---|
| Multi-Project TUI 004 — persistent restoration, resource bounds, and closure | Multi-Project TUI 003 is not closed | Implement and strictly close TUI 003, then activate the authored M4 plan | `plans/implementation/tui-project-sessions/004-persistent-restoration-resource-closure.md` |
| Session Projections 003 — visibility, redaction, and artifact handles | Session Projections 002 remains conditionally closed; principal/capability policy input seam is not yet verified | Implement the M2 corrective integration, strictly close M2, and verify the transport-derived capability seam | `plans/implementation/session-projections/003-visibility-redaction-artifact-handles.md` |
| Session Projections 004 — frontend adoption, compatibility, and closure | Projection M3 and Multi-Project TUI M3 are not closed | Close both prerequisite milestones without creating a second tab or reducer model | `plans/implementation/session-projections/004-frontend-adoption-compatibility-closure.md` |

## Recently closed work

| Subsystem | Milestone | Closure record | Closed at commit | Follow-up |
|---|---|---|---|---|
| Multi-project TUI and sessions | 002 — project picker and tab navigation | `plans/closure/tui-project-sessions/002-status.md` | `f569386` | `plans/implementation/tui-project-sessions/003-project-correct-event-routing-lifecycle.md` |
| Frontend-neutral session projections | 002 — scoped subscriptions and durable replay | `plans/closure/session-projections/002-status.md` | `8dc4b85` (library layer; conditional) | `plans/implementation/session-projections/002-corrective-daemon-integration-and-closure.md` |
| Frontend-neutral session projections | 001 — projection contracts and canonical reducer | `plans/closure/session-projections/001-status.md` | `f6c8669` (implementation) | Milestone 2 library and corrective integration |
| Multi-project TUI and sessions | 001 — project-aware state and catalog client | `plans/closure/tui-project-sessions/001-status.md` | `62e26b1` (implementation) | Milestone 2 closed; Milestone 3 ready |
| Project catalog and lazy discovery | 004 — protocol, server migration, and closure | `plans/closure/project-catalog/004-status.md` | `d1e5b70` (implementation) | TUI and projection consumers use the closed catalog protocol |
| Domain identity and compatibility | 004 — closure and legacy-removal criteria | `plans/closure/domain-identity/004-status.md` | `c4e9cf8` | Project Catalog 004 closed the remaining server locator criteria |
| Runtime assets and harness interoperability | 004 — immutable runtime pinning and closure | `plans/closure/runtime-assets/004-status.md` | `2293a11` | Project Catalog and TUI consumers use the closed generation/pinning contract |
| Provider connections and Eggpool | 005 — corrective lifecycle, rotation, health, and closure | `plans/closure/provider-connections/005-status.md` | `0eadc85` | — |

## Registry maintenance rules

1. Add a subsystem roadmap when it becomes active, not merely because it is a possible future track.
2. Register an implementation plan as dependency-ready only after dependency and handoff review.
3. Authored downstream plans remain in the blocked section until their activation criteria close.
4. Move a plan from ready to active when implementation begins.
5. Move it to closing when production work lands and closure review starts.
6. Mark it closed only when the linked closure record says closed.
7. Record blockers precisely and link the document that owns their resolution.
8. Remove closed rows from active sections after recording them under recently closed work.
9. Periodically move old closed interim documents to `plans/archive/` while preserving links.
10. Do not copy detailed milestone requirements into this registry.
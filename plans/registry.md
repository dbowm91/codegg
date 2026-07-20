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
| Multi-project TUI and sessions | active | `plans/subsystems/tui-project-sessions-roadmap.md` | Milestone 2 — project picker and tab navigation | Plan registered; ready for handoff |
| Frontend-neutral session projections | active | `plans/subsystems/session-projections-roadmap.md` | Milestone 2 — scoped subscriptions and durable replay | Implementation never landed; see `plans/closure/session-projections/002-status.md`. Milestone 3 blocked until M2 implementation resumes. |

## Dependency-ready implementation plans

| Subsystem | Milestone | Class | Plan | Baseline | Status |
|---|---|---|---|---|---|
| Multi-project TUI and sessions | 002 — project picker and tab navigation | capability | `plans/implementation/tui-project-sessions/002-project-picker-tab-navigation.md` | `1c37787` | ready |

## Active closure work

| Subsystem | Milestone | Closure record | Status | Open findings |
|---|---|---|---|---|
| — | — | — | — | None. |

## Blocked work

| Work item | Blocker | Required resolution | Owner document |
|---|---|---|---|
| Multi-Project TUI 003 — project-correct event routing and lifecycle | Multi-Project TUI 002 is not yet closed | Close TUI 002 and consume its picker/tab/session navigation ownership without creating a second tab model | `plans/subsystems/tui-project-sessions-roadmap.md` |
| Session Projections 003 — visibility, redaction, and artifact handles | Session Projections 002 implementation never landed (`plans/closure/session-projections/002-status.md`); future principal capability filtering seam remains required | Land Projection 002 production work against `plans/implementation/session-projections/002-scoped-subscriptions-durable-replay.md`, then define the fail-closed visibility/redaction and bounded artifact-read policy around the durable replay authority | `plans/subsystems/session-projections-roadmap.md` |

## Recently closed work

| Subsystem | Milestone | Closure record | Closed at commit | Follow-up |
|---|---|---|---|---|
| Frontend-neutral session projections | 001 — projection contracts and canonical reducer | `plans/closure/session-projections/001-status.md` | `f6c8669` (implementation) | `plans/implementation/session-projections/002-scoped-subscriptions-durable-replay.md` |
| Multi-project TUI and sessions | 001 — project-aware state and catalog client | `plans/closure/tui-project-sessions/001-status.md` | `62e26b1` (implementation) | `plans/implementation/tui-project-sessions/002-project-picker-tab-navigation.md` |
| Project catalog and lazy discovery | 004 — protocol, server migration, and closure | `plans/closure/project-catalog/004-status.md` | `d1e5b70` (implementation) | TUI 001 and Session Projections 001 are closed; their Milestone 002 handoffs are now registered |
| Domain identity and compatibility | 004 — closure and legacy-removal criteria | `plans/closure/domain-identity/004-status.md` | `c4e9cf8` | Project Catalog 004 closed the remaining server locator criteria |
| Domain identity and compatibility | 003 — corrective daemon and protocol adoption | `plans/closure/domain-identity/003-corrective-status.md` | `ec42dce` | Milestone 4 — closure and legacy-removal criteria |
| Runtime assets and harness interoperability | 004 — immutable runtime pinning and closure | `plans/closure/runtime-assets/004-status.md` | `2293a11` | Project Catalog and TUI consumers use the closed generation/pinning contract |
| Runtime assets and harness interoperability | 003 — refresh lifecycle and operator surface | `plans/closure/runtime-assets/003-status.md` | `972c286` | Project Catalog activation and Multi-Project TUI consume the refresh contract |
| Runtime assets and harness interoperability | 002 — explicit-context agent and instruction resolution | `plans/closure/runtime-assets/002-status.md` | `155f7f3` | Runtime Assets M003 |
| Runtime assets and harness interoperability | 001 — project asset registry | `plans/closure/runtime-assets/001-status.md` | `f9db5c3` | Milestone 2 — explicit-context agent and instruction resolution |
| Project catalog and lazy discovery | 003 — lazy activation and health | `plans/closure/project-catalog/003-status.md` | `27cbd43` (implementation) | Project Catalog 004 closed; Multi-Project TUI consumes the protocol |
| Project catalog and lazy discovery | 002 — bounded discovery and reconciliation | `plans/closure/project-catalog/002-status.md` | `5974976` (implementation) | Project Catalog M003 |
| Project catalog and lazy discovery | 001 — durable catalog foundation | `plans/closure/project-catalog/001-status.md` | `a2db5e4` | Milestone 2 — bounded discovery and reconciliation |
| Provider connections and Eggpool | 005 — corrective lifecycle, rotation, health, and closure | `plans/closure/provider-connections/005-status.md` | `0eadc85` | — |
| Provider connections and Eggpool | 003 — session/model selection by connection | `plans/closure/provider-connections/003-status.md` | `efe1995` | Provider roadmap closed after Milestone 5 |
| Domain identity and compatibility | 002 — project/repository storage migration | `plans/closure/domain-identity/002-status.md` | `84d92f0` | Runtime Assets 001 and Project Catalog 001 |
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

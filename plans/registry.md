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
| Multi-project TUI and sessions | closed | `plans/subsystems/tui-project-sessions-roadmap.md` | All milestones closed (001–004) | — |
| Frontend-neutral session projections | closed | `plans/subsystems/session-projections-roadmap.md` | Milestone 4 — frontend adoption and closure | Milestone 3 strictly closed; M4 is dependency-ready. |

## Dependency-ready implementation plans

| Subsystem | Milestone | Class | Plan | Baseline | Status |
|---|---|---|---|---|---|


## Active closure work

| Subsystem | Milestone | Closure record | Status | Open findings |
|---|---|---|---|---|

## Blocked work

| Work item | Blocker | Required resolution | Owner document |
|---|---|---|---|

## Recently closed work

| Subsystem | Milestone | Closure record | Closed at commit | Follow-up |
|---|---|---|---|---|
| Frontend-neutral session projections | 003 — visibility, redaction, and artifact handles | `plans/closure/session-projections/003-status.md` | WP A–D implementation + WP E verification | Milestone 004 (frontend adoption and closure) |
| Frontend-neutral session projections | 002 — scoped subscriptions and durable replay | `plans/closure/session-projections/002-status.md` | this commit (corrective daemon integration + library at `8dc4b85`) | Milestone 003 (visibility/redaction/artifact handles) — now closed |
| Frontend-neutral session projections | 001 — projection contracts and canonical reducer | `plans/closure/session-projections/001-status.md` | `f6c8669` (implementation) | `plans/implementation/session-projections/002-scoped-subscriptions-durable-replay.md` |
| Multi-project TUI and sessions | 004 — persistent restoration, resource bounds, and closure | `plans/closure/tui-project-sessions/004-status.md` | (this commit) | Subsystem roadmap closed; downstream plans are independent |
| Multi-project TUI and sessions | 003 — event routing and lifecycle | `plans/closure/tui-project-sessions/003-status.md` | (this commit) | `plans/implementation/tui-project-sessions/004-persistent-restoration-resource-closure.md` |
| Multi-project TUI and sessions | 002 — project picker and tab navigation | `plans/closure/tui-project-sessions/002-status.md` | (this commit) | `plans/implementation/tui-project-sessions/003-event-routing-lifecycle.md` |
| Multi-project TUI and sessions | 001 — project-aware state and catalog client | `plans/closure/tui-project-sessions/001-status.md` | `62e26b1` (implementation) | `plans/implementation/tui-project-sessions/002-project-picker-tab-navigation.md` |
| Project catalog and lazy discovery | 004 — protocol, server migration, and closure | `plans/closure/project-catalog/004-status.md` | `d1e5b70` (implementation) | TUI and projection consumers use the closed catalog protocol |
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

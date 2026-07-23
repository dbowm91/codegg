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
- **conditionally closed** — substantial work landed, but a named correctness finding prevents strict closure.
- **superseded** — replaced by another document.
- **archived** — no longer active and retained for traceability.

## Active subsystem roadmaps

| Subsystem | Status | Roadmap | Current milestone | Dependencies or blockers |
|---|---|---|---|---|
| Domain identity and compatibility | closed | `plans/subsystems/domain-identity-roadmap.md` | Milestone 4 closed | — |
| Runtime assets and harness interoperability | closed | `plans/subsystems/runtime-assets-roadmap.md` | Milestone 4 closed | — |
| Provider connections and Eggpool | closed | `plans/subsystems/provider-connections-roadmap.md` | Milestone 5 closed | — |
| Project catalog and lazy discovery | closed | `plans/subsystems/project-catalog-roadmap.md` | Milestone 4 closed | — |
| Multi-project TUI and sessions | closed | `plans/subsystems/tui-project-sessions-roadmap.md` | Milestones 001–004 closed | — |
| Frontend-neutral session projections | closed | `plans/subsystems/session-projections-roadmap.md` | Milestone 012 closed | — |

## Dependency-ready implementation plans

None — no registered Session Projections implementation plan is dependency-ready after M012 closure.

## Active closure work

None — no registered closure review is active after M012 acceptance.

## Blocked work

None — no registered milestone is blocked by the Session Projections M012 closure.

## Deferred unregistered product work

These are not dependency-ready correctness plans and remain outside the active handoff:

- cross-tab artifact hand-off UX;
- numeric acknowledgement/resync hot-key UX;
- plugin-specific `ProjectionEvent::PluginUi` semantics;
- final removal of legacy remote variants after the compatibility window;
- final team roles, presence, and chat.

## Recently closed or conditionally closed work

| Subsystem | Milestone | Closure record | Closed/reviewed at commit | Follow-up |
|---|---|---|---|---|
| Frontend-neutral session projections | 012 — TUI disconnect lifecycle and final evidence closure | `plans/closure/session-projections/012-status.md` | `0672044` implementation; `f046de5` corrective test evidence; final reviewed head `f046de5` | Closed; no registered future plan was newly unblocked, and deferred product work remains unregistered |
| Frontend-neutral session projections | 011 — evidence correctness and mechanism verification closure | `plans/closure/session-projections/011-status.md` | `560b8b7` main implementation; final reviewed head `1a93167` | Historical conditional closure; M012 accepted the remaining lifecycle, evidence, stability, and reconciliation work |
| Frontend-neutral session projections | 010 — mechanism-faithful transport verification and final closure | `plans/closure/session-projections/010-status.md` | `a3ab136` implementation; final reviewed M10 head `8bd59b2` | Historical conditional record; M011/M012 own final verification and lifecycle depth |
| Frontend-neutral session projections | 009 — production-shaped transport verification and strict closure | `plans/closure/session-projections/009-status.md` | `3406c742` implementation/evidence; `426dfffe` follow-up | Historical conditional record; M10–M12 own final depth |
| Frontend-neutral session projections | 008 — final transport lifecycle and replay evidence polish | `plans/closure/session-projections/008-status.md` | `6975050a` implementation; `ea6e38d` original closure | Historical conditional record; later milestones own final depth |
| Frontend-neutral session projections | 007 — corrective transport lifecycle and evidence closure | `plans/closure/session-projections/007-status.md` | `9887c2d` implementation; `922333b` original closure | Historical conditional record |
| Frontend-neutral session projections | 006 — atomic control delivery, transport verification, and raw compatibility hardening | `plans/closure/session-projections/006-status.md` | `270cc5f` closure; `8ca570f` implementation | Historical conditional record |
| Frontend-neutral session projections | 005 — remote transport isolation, resume, and compatibility closure | `plans/closure/session-projections/005-status.md` | `4c751ff` | M006 hardened atomic control delivery and normal-flow transport evidence |
| Frontend-neutral session projections | 004 — frontend adoption and compatibility | `plans/closure/session-projections/004-status.md` | `4c751ff` | — |
| Frontend-neutral session projections | 003 — visibility, redaction, and artifact handles | `plans/closure/session-projections/003-status.md` | `bac73ce` | — |
| Frontend-neutral session projections | 002 — scoped subscriptions and durable replay | `plans/closure/session-projections/002-status.md` | `c1d910a` corrective integration; library at `8dc4b85` | — |
| Frontend-neutral session projections | 001 — projection contracts and canonical reducer | `plans/closure/session-projections/001-status.md` | `f6c8669` | — |
| Multi-project TUI and sessions | 004 — persistent restoration, resource bounds, and closure | `plans/closure/tui-project-sessions/004-status.md` | `0d98576` | — |
| Multi-project TUI and sessions | 003 — event routing and lifecycle | `plans/closure/tui-project-sessions/003-status.md` | `6ad9952` closure completion; implementation at `248aa32` | — |
| Multi-project TUI and sessions | 002 — project picker and tab navigation | `plans/closure/tui-project-sessions/002-status.md` | `f569386` | — |
| Multi-project TUI and sessions | 001 — project-aware state and catalog client | `plans/closure/tui-project-sessions/001-status.md` | `62e26b1` | — |
| Project catalog and lazy discovery | 004 — protocol, server migration, and closure | `plans/closure/project-catalog/004-status.md` | `d1e5b70` | — |
| Domain identity and compatibility | 004 — closure and legacy-removal criteria | `plans/closure/domain-identity/004-status.md` | `c4e9cf8` | — |
| Runtime assets and harness interoperability | 004 — immutable runtime pinning and closure | `plans/closure/runtime-assets/004-status.md` | `2293a11` | — |
| Provider connections and Eggpool | 005 — corrective lifecycle, rotation, health, and closure | `plans/closure/provider-connections/005-status.md` | `0eadc85` | — |

## Registry maintenance rules

1. Add a subsystem roadmap when it becomes active, not merely because it is a possible future track.
2. Register an implementation plan as dependency-ready only after dependency and handoff review.
3. Move a plan from ready to active when implementation begins.
4. Move it to closing when production work lands and closure review starts.
5. Mark it closed only when the linked closure record says closed and no unresolved high/medium finding remains.
6. Use conditionally closed when a post-closure correctness finding invalidates a strict claim.
7. Record blockers precisely and link the document that owns their resolution.
8. Remove closed rows from active sections after recording them under recently closed work.
9. Periodically archive old closed interim documents while preserving links.
10. Do not copy detailed milestone requirements into this registry.
11. When one milestone closes, create/register only the next dependency-ready handoff.

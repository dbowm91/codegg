## Active subsystem roadmaps

| Subsystem | Status | Roadmap | Current milestone | Dependencies or blockers |
|---|---|---|---|---|
| Domain identity and compatibility | closed | `plans/subsystems/domain-identity-roadmap.md` | Milestone 4 closed | — |
| Runtime assets and harness interoperability | closed | `plans/subsystems/runtime-assets-roadmap.md` | Milestone 4 closed | — |
| Provider connections and Eggpool | closed | `plans/subsystems/provider-connections-roadmap.md` | Milestone 5 closed | — |
| Project catalog and lazy discovery | closed | `plans/subsystems/project-catalog-roadmap.md` | Milestone 4 closed | — |
| Multi-project TUI and sessions | closed | `plans/subsystems/tui-project-sessions-roadmap.md` | Milestones 001–004 closed | — |
| Frontend-neutral session projections | closed | `plans/subsystems/session-projections-roadmap.md` | Milestone 012 closed | — |
| Programmatic tool execution and Tool Programs | active | `plans/subsystems/tool-programs-correctness-closure-addendum.md` | Milestone 013 ready | M011 and M012 are historical conditional records; M013 owns persisted authority, transactional delivery, durable descendants, checkpoint/replay, complete artifacts, process-level evidence, and strict closure |

## Dependency-ready implementation plans

| Subsystem | Milestone | Status | Implementation plan | Dependencies |
|---|---|---|---|---|
| Programmatic tool execution and Tool Programs | 013 — production authority, descendant, delivery, and recovery closure | ready | `plans/implementation/tool-programs/013-production-authority-descendant-and-recovery-closure.md` | M001–M012 implementation present; native runtime only; no external provider required |

## Active closure work

| Subsystem | Milestone | Status | Closure record | Notes |
|---|---|---|---|---|

No Tool Programs closure record is active. M013 implementation must move to `closing` before an independent reviewer creates `plans/closure/tool-programs/013-status.md`.

## Blocked work

| Subsystem | Milestone | Status | Implementation plan | Blocker |
|---|---|---|---|---|

No registered downstream plan is blocked on M013. Strict Tool Programs subsystem closure is blocked by M013 itself.

## Deferred unregistered product work

These are not dependency-ready correctness plans and remain outside the active handoff:

- cross-tab artifact hand-off UX;
- numeric acknowledgement/resync hot-key UX;
- plugin-specific `ProjectionEvent::PluginUi` semantics;
- final removal of legacy remote variants after the compatibility window;
- final team roles, presence, and chat;
- production hosted Tool Program transport;
- full ACP product integration.

## Recently closed or conditionally closed work

| Subsystem | Milestone | Closure record | Closed/reviewed at commit | Follow-up |
|---|---|---|---|---|
| Programmatic tool execution and Tool Programs | 012 — authority, recovery, delivery, and child-ownership corrective implementation | `plans/closure/tool-programs/012-status.md` | `d056e42` implementation; production-path review and reconciliation at `16b923f` | Historical conditional implementation record; M013 owns strict production closure |
| Programmatic tool execution and Tool Programs | 011 — production correctness and ownership closure | `plans/closure/tool-programs/011-status.md` | `0ae1067` implementation; `705ae2c` original closure; post-closure review at `d71a5ee` | Historical conditional implementation record; remaining findings transferred through M012 to M013 |
| Programmatic tool execution and Tool Programs | 010 — harness, Eggpool, chaos, performance, and closure | `plans/closure/tool-programs/010-status.md` | `2f5e3d3` implementation; `b62686e` closure/reconciliation | Historical conditional closure; M011–M013 own final production correctness depth |
| Programmatic tool execution and Tool Programs | 009 — OpenAI Responses hosted-program adapter | `plans/closure/tool-programs/009-status.md` | HEAD implementation | Historical capability/library closure; production Tool Programs remain native-only |
| Programmatic tool execution and Tool Programs | 008 — background programs, projections, and parent notification | `plans/closure/tool-programs/008-status.md` | HEAD implementation | Historical closure; M013 owns transactional delivery and restart correctness |
| Programmatic tool execution and Tool Programs | 007 — build/test child-job composition | `plans/closure/tool-programs/007-status.md` | HEAD implementation | Historical closure; M013 owns durable lineage, scheduler descendant cancellation, reattachment, permit convergence, and artifacts |
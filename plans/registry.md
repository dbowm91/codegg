# CodeGG Active Planning Registry

This file is the compact control surface for active interim planning. Detailed requirements remain in source roadmaps and implementation plans; completed history remains in `plans/closure/`, subsystem roadmaps, archived plans, and Git history.

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
- **conditionally closed** — substantial work landed, but a named correctness or operational evidence condition remains.
- **superseded** — replaced by another document.
- **archived** — no longer active and retained for traceability.

## Active subsystem roadmaps

No runtime-safety subsystem roadmap remains active. Its roadmap is retained as
the durable workstream index with a conditionally closed status.

## Dependency-ready implementation plans

No runtime-safety implementation plan is currently dependency-ready.

## Active closure work

No runtime-safety closure work remains active. M008 is recorded under recently
closed work as conditionally closed because its named external evidence is not
available on this host.

## Blocked work

No registered runtime-safety work is blocked. The supported-Linux Landlock
fixture and final hosted verify remain evidence conditions for strict closure,
not unregistered implementation work.

## Execution order

1. Keep the supported-Linux Landlock fixture as an operational evidence item;
   run it on one Landlock-capable Linux host when available.
2. If the fixture and final hosted verify later pass, update the existing
   closure records with factual evidence and promote the roadmap to strict
   `closed`; do not create another implementation milestone for that evidence.

Promotion requires updating this registry rather than treating a blocked plan as implicitly ready.

## Workstream closure policy

Runtime-safety milestones use one compact closure record each:

- `plans/closure/runtime-safety-resource-footprint/001-status.md`
- `plans/closure/runtime-safety-resource-footprint/002-status.md`
- `plans/closure/runtime-safety-resource-footprint/003-status.md`
- `plans/closure/runtime-safety-resource-footprint/004-status.md`
- `plans/closure/runtime-safety-resource-footprint/005-status.md`
- `plans/closure/runtime-safety-resource-footprint/006-status.md`
- `plans/closure/runtime-safety-resource-footprint/007-status.md`
- `plans/closure/runtime-safety-resource-footprint/008-status.md`
- `plans/closure/runtime-safety-resource-footprint/009-status.md` — corrective C001
- `plans/closure/runtime-safety-resource-footprint/009-m003-promotion-disposition.md` — dependency classification correction; not a new milestone

M001 requires independent security review. M002 requires a second correctness review of output bounds and process-tree cleanup. Corrective C001 requires independent security review of helper discovery, setup-status isolation, and fail-closed behavior. Those reviews are recorded as accepted for production implementation. Ordinary successful closure of M003–M008 does not require a separate ratification plan or evidence-transfer milestone. A reproducible high/medium defect receives one narrow corrective plan linked to its owning milestone.

Verification follows the existing minimal contract: focused mechanism checks, one `scripts/verify.sh quick` result on the accepted executable revision, one supported-Linux fixture run for strict C001/M001 closure, and one existing hosted `verify` result on the final combined PR revision when the normal trigger is available. Runner cache/post-step failures after verification succeeds and absence of a manual workflow trigger are operational evidence conditions, not reasons to duplicate implementation milestones. No duplicate local full-workspace run, release/package evidence, new CI lane, matrix, artifact, benchmark gate, or automated publication is required.

## Recently closed subsystem lines

These rows preserve only the latest closed control points. Detailed predecessor history remains in their roadmaps and closure directories.

| Subsystem | Status | Latest controlling document | Closure |
|---|---|---|---|
| Runtime safety, resource control, and footprint | conditionally closed | `plans/subsystems/runtime-safety-resource-footprint-roadmap.md` | `plans/closure/runtime-safety-resource-footprint/008-status.md` |
| Agent runtime, model adaptation, and ACP | closed | `plans/subsystems/agent-runtime-model-adaptation-acp-corrective-closure-addendum.md` | `plans/closure/agent-runtime-model-adaptation-acp/017-status.md` |
| Programmatic tool execution and Tool Programs | closed | `plans/subsystems/provider-tool-dvr-independent-closure-ratification-addendum.md` | `plans/closure/tool-programs/019-status.md`; corrective M020 also closed |
| Provider connections and Eggpool | closed | `plans/subsystems/provider-tool-dvr-independent-closure-ratification-addendum.md` | `plans/closure/provider-connections/007-status.md` |
| Development verification and release | closed | `plans/subsystems/development-verification-release-final-evidence-closure-addendum.md` | `plans/closure/development-verification-release/007-status.md` |
| Domain identity and compatibility | closed | `plans/subsystems/domain-identity-roadmap.md` | Latest closure linked from source roadmap |
| Runtime assets and harness interoperability | closed | `plans/subsystems/runtime-assets-roadmap.md` | Latest closure linked from source roadmap |
| Project catalog and lazy discovery | closed | `plans/subsystems/project-catalog-roadmap.md` | Latest closure linked from source roadmap |
| Multi-project TUI and sessions | closed | `plans/subsystems/tui-project-sessions-roadmap.md` | Latest closure linked from source roadmap |
| Frontend-neutral session projections | closed | `plans/subsystems/session-projections-roadmap.md` | Latest closure linked from source roadmap |

## Deferred unregistered product work

These remain outside the active correctness/footprint handoff until a concrete product priority or dependency makes them ready:

- cross-tab artifact hand-off UX;
- numeric acknowledgement/resync hot-key UX;
- plugin-specific `ProjectionEvent::PluginUi` semantics;
- final removal of legacy remote variants after the compatibility window;
- final team roles, presence, and chat;
- production hosted Tool Program transport;
- seccomp, namespace, container, or remote-execution sandbox expansion;
- persistent search indexing;
- automatic dependency-update bots or continuous binary-size gates;
- release automation or a fixed release cadence.

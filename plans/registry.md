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

| Subsystem | Status | Roadmap | Current milestone | Dependencies or blockers |
|---|---|---|---|---|
| Runtime safety, resource control, and footprint | closing | `plans/subsystems/runtime-safety-resource-footprint-roadmap.md` | Corrective C002 ready; M003/M004/M007 closed; M001/M002/M005/M006/M008/C001 conditionally closed | Production roadmap implementation is complete. C002 owns branch reconciliation, truthful PR metadata, one final hosted verify, one supported-Linux Landlock result, and the UTF-8 argv documentation disposition. |

## Dependency-ready implementation plans

| Subsystem | Milestone | Status | Implementation plan | Dependencies |
|---|---|---|---|---|
| Runtime safety, resource control, and footprint | Corrective C002 — final integration and evidence closure | ready | `plans/implementation/runtime-safety-resource-footprint/010-final-integration-and-evidence-closure.md` | M001–M008/C001 production dispositions accepted; PR #72 branch integration and two external evidence items remain. |

## Active closure work

No implementation is active yet. C002 is ready for handoff and is the sole registered runtime-safety item.

## Blocked work

No registered runtime-safety plan is blocked. C002 can begin immediately.

The supported-Linux Landlock fixture and final hosted verify are named C002 acceptance criteria. They are not reasons to add another workflow lane, evidence-transfer milestone, or release process.

## Execution order

1. Execute C002 by merging current `main` into `planning/runtime-safety-resource-footprint` without force-rewriting the accepted 41-commit evidence chain.
2. Resolve duplicated planning history by retaining the latest closure state and preserve accepted production code unless `main` contains a newer independent correction.
3. Correct PR #72 title/body to represent the complete M001–M008/C001 workstream and keep it draft until the reconciled head is stable.
4. Use the existing normal PR-triggered `verify` workflow on the final reconciled head; do not add a workflow lane or matrix.
5. Run the existing `sandbox_landlock` fixture once on a Landlock-capable Linux kernel, reusing the hosted run when it supplies real enforcement, kernel, ABI, and fixture evidence.
6. Qualify M003 documentation to state that the current typed command model is lossless for the supported UTF-8 representation, not arbitrary non-UTF-8 Unix argv, unless source review proves an existing stronger public contract.
7. Reconcile M001/M002/M003/C001/M008 closure records, add `plans/closure/runtime-safety-resource-footprint/010-status.md`, and promote the roadmap to strict `closed` only when the named evidence exists.
8. If Linux evidence alone remains unavailable, leave C002/M008 conditionally closed on that exact item and do not create C003.

Promotion requires updating this registry rather than treating a conditionally closed workstream as implicitly complete.

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
- `plans/closure/runtime-safety-resource-footprint/009-m003-promotion-disposition.md` — dependency classification correction; not a milestone
- `plans/closure/runtime-safety-resource-footprint/010-status.md` — final integration/evidence closure target

M001 requires independent security review. M002 requires a second correctness review of output bounds and process-tree cleanup. Corrective C001 requires independent security review of helper discovery, setup-status isolation, and fail-closed behavior. Those reviews are already accepted for production implementation.

C002 requires no additional ratification plan. A reproducible hosted-test or Linux-enforcement failure is handled as a concrete defect within C002. Unavailable external evidence may leave one conditional closure record but must not create another plan.

Verification remains minimal: focused mechanism/static checks after conflict resolution, one `scripts/verify.sh quick` result on the reconciled executable revision, one existing hosted `verify` run on the final PR head, and one supported-Linux fixture execution. No duplicate local full-workspace run, new CI lane, matrix, artifact, benchmark gate, continuous size gate, automated publication, or release cadence is required.

## Recently closed subsystem lines

These rows preserve only the latest closed control points. Detailed predecessor history remains in their roadmaps and closure directories.

| Subsystem | Status | Latest controlling document | Closure |
|---|---|---|---|
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

- arbitrary non-UTF-8 command/protocol transport unless elevated by an explicit supported-platform contract;
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

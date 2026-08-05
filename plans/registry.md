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
| Runtime safety, resource control, and footprint | active | `plans/subsystems/runtime-safety-resource-footprint-roadmap.md` | Corrective C001 conditionally closed; M006/M005 conditionally closed; M004 closed | C001 production correctness is accepted, but one supported-Linux enforcement result remains before M003 can be promoted; M007–M008 remain dependency ordered below |

## Dependency-ready implementation plans

| Subsystem | Milestone | Status | Implementation plan | Dependencies |
|---|---|---|---|---|

## Runtime-safety milestone dispositions

| Milestone | Status | Closure | Promotion decision |
|---|---|---|---|
| M001 — Landlock and sandbox contract correction | conditionally closed | `plans/closure/runtime-safety-resource-footprint/001-status.md` | Corrective C001 owns trusted helper resolution, isolated setup status, read-only-cwd plumbing, and supported-Linux evidence before M003 promotion |
| M002 — canonical bounded process execution | conditionally closed | `plans/closure/runtime-safety-resource-footprint/002-status.md` | Production implementation landed in `6e5fbfd`; its executable/argv interface is accepted, while C001 corrects the sandbox helper boundary before downstream promotion |
| M004 — grep concurrency and context efficiency | closed | `plans/closure/runtime-safety-resource-footprint/004-status.md` | Included as a soft input to final M007 measurements |
| M005 — dependency feature and namespace normalization | conditionally closed | `plans/closure/runtime-safety-resource-footprint/005-status.md` | Production and hosted verification steps passed; runner cache/post-step disk exhaustion is operational evidence and is not an independent M007 implementation blocker |
| M006 — deprecated parser and dependency maintenance | conditionally closed | `plans/closure/runtime-safety-resource-footprint/006-status.md` | Production and local full verification are accepted; unavailable exact-revision hosted dispatch is operational evidence and is not an independent M007 implementation blocker |
| Corrective C001 — sandbox helper trust channel and roadmap unblock | conditionally closed | `plans/closure/runtime-safety-resource-footprint/009-status.md` | Production correction and independent security review passed; supported-Linux enforcement evidence remains before strict closure |

## Blocked work

| Subsystem | Milestone | Status | Implementation plan | Blocker |
|---|---|---|---|---|
| Runtime safety, resource control, and footprint | M003 — typed argv and shell-routing convergence | blocked | `plans/implementation/runtime-safety-resource-footprint/003-typed-argv-and-shell-routing-convergence.md` | C001 production correction is complete, but its required supported-Linux enforcement result is not yet recorded; no separate per-milestone hosted rerun is required for promotion |
| Runtime safety, resource control, and footprint | M007 — binary topology and footprint reduction | blocked | `plans/implementation/runtime-safety-resource-footprint/007-binary-topology-and-footprint-reduction.md` | M003 must close; M002/M005/M006 production dispositions are accepted and M004 remains a soft final-measurement input |
| Runtime safety, resource control, and footprint | M008 — planning, verification, and maintenance closure | blocked | `plans/implementation/runtime-safety-resource-footprint/008-planning-verification-and-maintenance-closure.md` | Corrective C001 and M001–M007 must have accepted dispositions and compact closure records |

## Execution order

1. Execute corrective C001.
2. Close C001 only after trusted helper resolution, private setup-status transport, read-only-cwd behavior, focused verification, independent security review, and one supported-Linux Landlock run pass. C001 is conditionally closed while that run is unavailable.
3. Promote M003 to `ready` immediately after C001 closure; do not wait for duplicate cache-post-step or unavailable manual-dispatch evidence.
4. Promote M007 after M003 closes. Use the accepted M002/M004/M005/M006 production state for representative measurements.
5. Promote M008 after C001 and M001–M007 have accepted dispositions.

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

M001 requires independent security review. M002 requires a second correctness review of output bounds and process-tree cleanup. Corrective C001 requires independent security review of helper discovery, setup-status isolation, and fail-closed behavior. Ordinary successful closure of M003–M008 does not require a separate ratification plan or evidence-transfer milestone. A reproducible high/medium defect receives one narrow corrective plan linked to its owning milestone.

Verification follows the existing minimal contract: focused mechanism checks, one `scripts/verify.sh quick` result on the accepted executable revision, one supported-Linux fixture run where C001 requires it, and one existing hosted `verify` result on the final combined PR revision when the normal trigger is available. Runner cache/post-step failures after verification succeeds and absence of a manual workflow trigger are operational evidence conditions, not reasons to duplicate implementation milestones. No duplicate local full-workspace run, release/package evidence, new CI lane, matrix, artifact, benchmark gate, or automated publication is required.

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

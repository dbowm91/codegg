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
- **conditionally closed** — substantial work landed, but a named correctness finding prevents strict closure.
- **superseded** — replaced by another document.
- **archived** — no longer active and retained for traceability.

## Active subsystem roadmaps

| Subsystem | Status | Roadmap | Current milestone | Dependencies or blockers |
|---|---|---|---|---|
| Runtime safety, resource control, and footprint | active | `plans/subsystems/runtime-safety-resource-footprint-roadmap.md` | M001, M004, and M005 ready | M002–M003 and M006–M008 are dependency ordered below |

## Dependency-ready implementation plans

| Subsystem | Milestone | Status | Implementation plan | Dependencies |
|---|---|---|---|---|
| Runtime safety, resource control, and footprint | M001 — Landlock and sandbox contract correction | ready | `plans/implementation/runtime-safety-resource-footprint/001-landlock-and-sandbox-contract-correction.md` | None; highest-priority security correction; independent closure review required |
| Runtime safety, resource control, and footprint | M004 — grep concurrency and context efficiency | ready | `plans/implementation/runtime-safety-resource-footprint/004-grep-concurrency-and-context-efficiency.md` | None; may execute in parallel with M001 |
| Runtime safety, resource control, and footprint | M005 — dependency feature and namespace normalization | ready | `plans/implementation/runtime-safety-resource-footprint/005-dependency-feature-and-namespace-normalization.md` | None; may execute in parallel with M001 and M004 |

## Blocked work

| Subsystem | Milestone | Status | Implementation plan | Blocker |
|---|---|---|---|---|
| Runtime safety, resource control, and footprint | M002 — canonical bounded process execution | blocked | `plans/implementation/runtime-safety-resource-footprint/002-canonical-bounded-process-execution.md` | Hard dependency on M001's accepted sandbox request/outcome contract |
| Runtime safety, resource control, and footprint | M003 — typed argv and shell-routing convergence | blocked | `plans/implementation/runtime-safety-resource-footprint/003-typed-argv-and-shell-routing-convergence.md` | Hard dependency on M002's canonical executor |
| Runtime safety, resource control, and footprint | M006 — deprecated parser and dependency maintenance | blocked | `plans/implementation/runtime-safety-resource-footprint/006-deprecated-parser-and-dependency-maintenance.md` | M005 must stabilize dependency/manifest ownership before parser migration |
| Runtime safety, resource control, and footprint | M007 — binary topology and footprint reduction | blocked | `plans/implementation/runtime-safety-resource-footprint/007-binary-topology-and-footprint-reduction.md` | Hard dependencies on M002, M003, M005, and M006; M004 is a soft final-measurement dependency |
| Runtime safety, resource control, and footprint | M008 — planning, verification, and maintenance closure | blocked | `plans/implementation/runtime-safety-resource-footprint/008-planning-verification-and-maintenance-closure.md` | M001–M007 must have accepted dispositions and compact closure records |

## Execution order

1. Execute M001 first.
2. M004 and M005 may execute independently in parallel with M001.
3. Promote M002 after M001 closes.
4. Promote M003 after M002 closes.
5. Promote M006 after M005 stabilizes manifest ownership.
6. Promote M007 after M002, M003, M005, and M006 close; include M004 in final measurements when available.
7. Promote M008 after all production milestones have accepted dispositions.

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

M001 requires independent security review. M002 requires a second correctness review of output bounds and process-tree cleanup. Ordinary successful closure of M003–M008 does not require a separate ratification plan or evidence-transfer milestone. A reproducible high/medium defect receives one narrow corrective plan linked to its owning milestone.

Verification follows the existing minimal contract: focused mechanism checks, one `scripts/verify.sh quick` result on the accepted executable revision, and one existing hosted `verify` result where applicable. No duplicate local full-workspace run, release/package evidence, new CI lane, matrix, artifact, benchmark gate, or automated publication is required.

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

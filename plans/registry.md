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
| Post-audit correctness, simplification, and footprint | closing | `plans/subsystems/post-audit-correctness-simplification-roadmap.md` | M002 closure review; M003-M007 ready; M008 blocked on M002-M007 | M001 is closed. M002 production implementation is landed; closure evidence is in progress. Preserve single daemon, single binary, manual release, and one-job CI posture. |
| Runtime safety, resource control, and footprint | conditionally closed | `plans/subsystems/runtime-safety-resource-footprint-roadmap.md` | C002 conditionally closed | Production implementation is merged. Only the previously named supported-Linux Landlock fixture evidence remains; it is independent of the post-audit workstream. |

## Dependency-ready implementation plans

| Subsystem | Milestone | Status | Implementation plan | Dependencies |
|---|---|---|---|---|
| Post-audit correctness, simplification, and footprint | M003 | ready | `plans/implementation/post-audit-correctness-simplification/003-tui-text-layout-correctness-and-render-deduplication.md` | none |
| Post-audit correctness, simplification, and footprint | M004 | ready | `plans/implementation/post-audit-correctness-simplification/004-dependency-feature-slimming-and-upstream-review.md` | none; soft final-measurement dependency on M003 when it lands first |
| Post-audit correctness, simplification, and footprint | M005 | ready | `plans/implementation/post-audit-correctness-simplification/005-routine-ci-and-static-guard-simplification.md` | none; reconcile final stack env with M006 |
| Post-audit correctness, simplification, and footprint | M006 | ready | `plans/implementation/post-audit-correctness-simplification/006-test-stack-and-resource-root-cause-correction.md` | none; soft CI reconciliation with M005 |
| Post-audit correctness, simplification, and footprint | M007 | ready | `plans/implementation/post-audit-correctness-simplification/007-execution-model-pass-through-cleanup.md` | none |

## Active closure work

M002 is in closure review at `plans/closure/post-audit-correctness-simplification/002-status.md`. M001 is closed in `plans/closure/post-audit-correctness-simplification/001-status.md`.

Runtime-safety C002 remains conditionally closed only on its previously recorded supported-Linux Landlock fixture evidence in `plans/closure/runtime-safety-resource-footprint/010-status.md`. Do not create another runtime-safety milestone for that external evidence item.

## Blocked work

| Subsystem | Milestone | Status | Implementation plan | Blocker |
|---|---|---|---|---|
| Post-audit correctness, simplification, and footprint | M008 | blocked | `plans/implementation/post-audit-correctness-simplification/008-integration-measurement-and-closure.md` | hard dependency on closure of M002-M007 |

No M001-M007 milestone is blocked at registration time.

## Execution order

The post-audit plans are independently executable, but the preferred handoff order prioritizes security/correctness before polish:

1. M001 — correct untrusted HTTP output limits, bounded streaming, and actual-address SSRF pinning.
2. M002 — make daemon stop identity-safe and replace handwritten CLI JSON escaping.
3. M003 — correct multiline reasoning-tag scanning, Unicode wrapping/counting, and ShareDialog render duplication.
4. M004 — apply measured no-feature-loss dependency/default-feature slimming and upstream review.
5. M005 — remove invalid/redundant routine CI and static-guard machinery while retaining high-value checks.
6. M006 — reproduce and correct the daemon-socket stack issue, then remove the global 32 MiB stack workaround.
7. M007 — collapse only execution-model pass-through layers that prove to own no distinct invariant.
8. M008 — integrate, measure, obtain one final hosted verify result, reconcile docs/registry, and close the workstream.

M001-M007 may be implemented in parallel only when branch coordination prevents overlapping edits. M005 and M006 both touch CI/testing documentation and should be reconciled carefully if executed concurrently.

The runtime-safety supported-Linux Landlock evidence condition may be collected independently at any time. It does not block M001-M008 and must not cause this roadmap to absorb another sandbox implementation phase.

## Workstream closure policy

Post-audit milestones use one compact closure record each under:

- `plans/closure/post-audit-correctness-simplification/001-status.md`
- `plans/closure/post-audit-correctness-simplification/002-status.md`
- `plans/closure/post-audit-correctness-simplification/003-status.md`
- `plans/closure/post-audit-correctness-simplification/004-status.md`
- `plans/closure/post-audit-correctness-simplification/005-status.md`
- `plans/closure/post-audit-correctness-simplification/006-status.md`
- `plans/closure/post-audit-correctness-simplification/007-status.md`
- `plans/closure/post-audit-correctness-simplification/008-status.md`

M001 requires explicit security evidence for address pinning and bounded body collection. M002 requires lifecycle evidence that stale/mismatched daemon metadata cannot signal an unrelated PID. M006 requires evidence from the previously failing stack path with the global override unset. M008 owns the single final broad integration/measurement pass.

Verification remains minimal: focused tests for each changed boundary, `scripts/verify.sh quick` for production/manifests, and one existing hosted `verify` run on the integrated final head. No new CI lane, matrix, artifact, benchmark/coverage/size gate, scheduled audit, automatic publication, or release cadence is required.

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
| Runtime safety, resource control, and footprint | conditionally closed | `plans/subsystems/runtime-safety-resource-footprint-roadmap.md` | `plans/closure/runtime-safety-resource-footprint/010-status.md` |

## Deferred unregistered product work

These remain outside the active corrective/simplification handoff until a concrete product priority or new evidence makes them ready:

- arbitrary non-UTF-8 command/protocol transport unless elevated by an explicit supported-platform contract;
- binary topology split or separate daemon/TUI packaging without new measured deployment need;
- replacing RustPython with a custom Tool Program parser;
- generalized HTTP/provider-client unification;
- broad Comrak/MSRV migration;
- cross-tab artifact hand-off UX;
- numeric acknowledgement/resync hot-key UX;
- plugin-specific `ProjectionEvent::PluginUi` semantics;
- final removal of legacy remote variants after the compatibility window;
- final team roles, presence, and chat;
- production hosted Tool Program transport;
- seccomp, namespace, container, or remote-execution sandbox expansion;
- persistent search indexing;
- automatic dependency-update bots or continuous binary-size/audit gates;
- release automation or a fixed release cadence.

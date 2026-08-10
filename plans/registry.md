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
| Post-audit correctness, simplification, and footprint | blocked | `plans/subsystems/post-audit-correctness-simplification-corrective-closure-addendum.md` | C001 blocked; C002 ready | PR #73 is merged, but hosted `verify` exposes a pre-existing `/dev/null` sandbox-rights defect; C002 owns the fix. |
| Runtime safety, resource control, and footprint | conditionally closed | `plans/subsystems/runtime-safety-resource-footprint-roadmap.md` | C002 conditionally closed | Production implementation is merged. Only the previously named supported-Linux Landlock fixture evidence remains; it is independent of C001. |

## Dependency-ready implementation plans

| Subsystem | Milestone | Status | Implementation plan | Dependencies |
|---|---|---|---|---|
| Post-audit correctness, simplification, and footprint | C002 | ready | `plans/implementation/post-audit-correctness-simplification/011-sandbox-rights-correction-and-strict-closure.md` | Concrete sandbox setup defect identified by C001 hosted verification; no product predecessor required |

## Active closure work

C001 is blocked after merging PR #73 because hosted verification exposed a concrete sandbox setup defect. C002 is the narrowly scoped corrective implementation and strict-closure owner; it must not reopen accepted M001-M008 production scope.

Source addendum:

- `plans/subsystems/post-audit-correctness-simplification-corrective-closure-addendum.md`

Historical C001 blocker record:

- `plans/closure/post-audit-correctness-simplification/009-corrective-status.md`

Target C002 closure:

- `plans/closure/post-audit-correctness-simplification/010-sandbox-rights-correction-status.md`

Runtime-safety C002 remains conditionally closed only on its previously recorded supported-Linux Landlock fixture evidence in `plans/closure/runtime-safety-resource-footprint/010-status.md`. Do not create another runtime-safety milestone for that external evidence item.

## Blocked work

| Subsystem | Milestone | Blocker | Resolution owner |
|---|---|---|---|
| Post-audit correctness, simplification, and footprint | C001 | Hosted `verify` fails seven Python-script sandbox tests when `/dev/null` receives directory-only rights | C002 at `plans/implementation/post-audit-correctness-simplification/011-sandbox-rights-correction-and-strict-closure.md` |

## Execution order

1. Capture the exact seven Python-script executor failures from hosted run `31266908787` and confirm the current `src/security/sandbox.rs::add_landlock_path_rule` diagnosis still applies.
2. Correct Landlock access-mask construction on the directory/non-directory boundary so special non-directory paths such as `/dev/null` do not receive `ReadDir`, without broadening sandbox authority.
3. Add focused directory, regular-file, and special-file regression coverage, then rerun the seven previously failing executor tests and existing sandbox/security guards.
4. Run `scripts/verify.sh quick` and one normal existing hosted `verify` job on the actual merge candidate. Do not add CI lanes, matrices, artifacts, audit/coverage/size gates, or release automation.
5. Only after hosted `verify` is green, create the C002 closure record, reconcile C001 from blocked to closed, remove C002 from dependency-ready work, and return this post-audit workstream to strict `closed` state.

Detailed handoff:

- `plans/implementation/post-audit-correctness-simplification/011-sandbox-rights-correction-and-strict-closure.md`

The original `010-sandbox-file-rights-correction.md` registration stub is superseded and retained only for planning history.

The independent runtime-safety supported-Linux Landlock evidence condition may be collected at any time. It does not block this corrective pass and must not be folded into C002.

## Workstream closure policy

M001-M008 remain closed as implementation milestones with their existing historical records:

- `plans/closure/post-audit-correctness-simplification/001-status.md`
- `plans/closure/post-audit-correctness-simplification/002-status.md`
- `plans/closure/post-audit-correctness-simplification/003-status.md`
- `plans/closure/post-audit-correctness-simplification/004-status.md`
- `plans/closure/post-audit-correctness-simplification/005-status.md`
- `plans/closure/post-audit-correctness-simplification/006-status.md`
- `plans/closure/post-audit-correctness-simplification/007-status.md`
- `plans/closure/post-audit-correctness-simplification/008-status.md`

C001 is a corrective integration/closure pass, not M009. File number `009` preserves sequential filenames. Its closure record preserves the fact that M008's production evidence was accepted before the PR was merged. C002 owns only the concrete hosted sandbox-rights blocker and the resulting strict closure reconciliation.

Verification remains minimal: focused tests for the actual sandbox correction, existing sandbox/security guards, `scripts/verify.sh quick`, and one existing hosted `verify` run on the merge candidate. No duplicate local full-workspace run, new CI lane, matrix, artifact, benchmark/coverage/size gate, scheduled audit, automatic publication, or release cadence is required.

## Recently closed implementation plans

| Subsystem | Milestone | Status | Closure | Implementation commit |
|---|---|---|---|---|
| Post-audit correctness, simplification, and footprint | C001 — corrective PR integration | blocked | `plans/closure/post-audit-correctness-simplification/009-corrective-status.md` | `8a556f05`; C002 required for hosted sandbox test gate |
| Post-audit correctness, simplification, and footprint | M008 — integration, measurement, and closure | closed | `plans/closure/post-audit-correctness-simplification/008-status.md` | PR #73 production closure head; C001 reconciled merge state |
| Post-audit correctness, simplification, and footprint | M007 — execution-model pass-through cleanup | closed | `plans/closure/post-audit-correctness-simplification/007-status.md` | `17e1f5a` |
| Post-audit correctness, simplification, and footprint | M006 — test stack and resource-root-cause correction | closed | `plans/closure/post-audit-correctness-simplification/006-status.md` | `a4402db` |
| Post-audit correctness, simplification, and footprint | M005 — routine CI and static-guard simplification | closed | `plans/closure/post-audit-correctness-simplification/005-status.md` | `0993d953` |
| Post-audit correctness, simplification, and footprint | M004 — dependency feature slimming and upstream maintenance review | closed | `plans/closure/post-audit-correctness-simplification/004-status.md` | `b437f8eb` |

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
- broad Ratatui/TUI dependency migration unless a future dependency-maintenance priority explicitly activates it;
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

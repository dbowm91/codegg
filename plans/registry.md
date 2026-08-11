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
| Agent runtime correctness, autonomy, and simplification | conditionally closed | `plans/subsystems/agent-runtime-correctness-autonomy-simplification-corrective-closure-addendum.md` | M010 conditionally closed | Production correction is pushed at `cbdc0150`; exact hosted `verify` evidence remains unavailable because PR #74 reports no checks for the branch. |
| Post-audit correctness, simplification, and footprint | closed | `plans/subsystems/post-audit-correctness-simplification-corrective-closure-addendum.md` | C002 closed | C002 corrected the `/dev/null` Landlock path-rights defect; hosted `verify` run `31425564638` passed on the actual merge candidate. |
| Runtime safety, resource control, and footprint | conditionally closed | `plans/subsystems/runtime-safety-resource-footprint-roadmap.md` | C002 conditionally closed | Production implementation is merged. Only the previously named supported-Linux Landlock fixture evidence remains; it is independent of the agent-runtime corrective closure pass. |

## Dependency-ready implementation plans

| Subsystem | Milestone | Status | Implementation plan | Dependencies |
|---|---|---|---|---|
M010 is conditionally closed by `plans/closure/agent-runtime-correctness-autonomy-simplification/010-status.md`. The production correction is complete; only the exact hosted `CI / verify` result remains to be attached if GitHub exposes a normal PR check for the pushed candidate.

PR #74 / M009 remains useful predecessor integration evidence, including the broker-principal correction, explicit workspace fixture reconciliation, documentation updates, and green predecessor hosted run `31515706555`. Its historical `closed` claim is reconciled by M010's corrective record.

The previously completed post-audit corrective line remains closed. Historical control points are retained in:

- `plans/subsystems/post-audit-correctness-simplification-corrective-closure-addendum.md`
- `plans/closure/post-audit-correctness-simplification/009-corrective-status.md`
- `plans/closure/post-audit-correctness-simplification/010-sandbox-rights-correction-status.md`

Runtime-safety C002 remains conditionally closed only on its previously recorded supported-Linux Landlock fixture evidence in `plans/closure/runtime-safety-resource-footprint/010-status.md`. Do not create another runtime-safety milestone for that external evidence item.

## Blocked work

No registered implementation plans are currently blocked by the agent-runtime corrective closure pass.

## Execution order

The controlling sequence for this workstream is now:

1. M001-M008 remain closed and are not reopened except for focused regression verification at their integration boundaries.
2. M009 / PR #74 is predecessor integration work. Retain or rebase its valid broker-principal, workspace-fixture, project-catalog-guard, and documentation corrections.
3. M010 is conditionally closed: it deleted unreachable recovery code, removed the unbudgeted repository continuation path, unified primary/follow-up continuation authority, and added the typed recovery boundary.
4. The exact hosted `CI / verify` result remains the named operational condition for strict closure.

Verification remains minimal and change-specific: focused recovery/loop/harness tests, `scripts/verify.sh quick`, and one ordinary hosted `verify` run on the final corrective candidate. Do not add a new CI lane, matrix, dead-code guard, scheduled audit, artifact workflow, coverage/benchmark/size gate, automatic publication, or release cadence.

## Workstream closure policy

The active agent-runtime correctness workstream now closes only through M010 and `plans/closure/agent-runtime-correctness-autonomy-simplification/010-status.md`.

M009 remains historical integration/closure-attempt evidence. Its PR and green candidate are not discarded, but its final closure recommendation is superseded by the corrective requirement recorded in:

- `plans/subsystems/agent-runtime-correctness-autonomy-simplification-corrective-closure-addendum.md`;
- `plans/implementation/agent-runtime-correctness-autonomy-simplification/010-recovery-state-strict-closure-corrective-pass.md`.

M001-M008 retain their existing closure records. M010 must not rewrite those records to conceal the discrepancy; it must add corrective traceability and demonstrate that the remaining recovery-state acceptance criteria are actually true on the final tree.

The earlier post-audit M001-M008 remain closed as historical implementation milestones with their existing records:

- `plans/closure/post-audit-correctness-simplification/001-status.md`
- `plans/closure/post-audit-correctness-simplification/002-status.md`
- `plans/closure/post-audit-correctness-simplification/003-status.md`
- `plans/closure/post-audit-correctness-simplification/004-status.md`
- `plans/closure/post-audit-correctness-simplification/005-status.md`
- `plans/closure/post-audit-correctness-simplification/006-status.md`
- `plans/closure/post-audit-correctness-simplification/007-status.md`
- `plans/closure/post-audit-correctness-simplification/008-status.md`

Historical post-audit C001/C002 remain corrective closure passes, not milestones in the new workstream.

## Recently closed implementation plans

| Subsystem | Milestone | Status | Closure | Implementation commit |
|---|---|---|---|---|
| Agent runtime correctness, autonomy, and simplification | M010 — recovery-state strict closure corrective pass | conditionally closed | `plans/closure/agent-runtime-correctness-autonomy-simplification/010-status.md` | `cbdc0150`; hosted final run unavailable |
| Agent runtime correctness, autonomy, and simplification | M007 — measured binary footprint and upstream dependency review | closed | `plans/closure/agent-runtime-correctness-autonomy-simplification/007-status.md` | `deb07a2` |
| Agent runtime correctness, autonomy, and simplification | M008 — routine CI and static-guard contraction | closed | `plans/closure/agent-runtime-correctness-autonomy-simplification/008-status.md` | `66326ad` |
| Agent runtime correctness, autonomy, and simplification | M006 — prompt compilation and control-policy consolidation | closed | `plans/closure/agent-runtime-correctness-autonomy-simplification/006-status.md` | `4cd004d` |
| Agent runtime correctness, autonomy, and simplification | M004 — turn identity, accounting, and lifecycle correctness | closed | `plans/closure/agent-runtime-correctness-autonomy-simplification/004-status.md` | `493fd59` |
| Agent runtime correctness, autonomy, and simplification | M005 — recovery and autonomy state machine | closed | `plans/closure/agent-runtime-correctness-autonomy-simplification/005-status.md`; M010 corrective reconciliation required | `ddb495a` |
| Agent runtime correctness, autonomy, and simplification | M003 — workspace-bound AgentLoop construction | closed | `plans/closure/agent-runtime-correctness-autonomy-simplification/003-status.md` | `8c2638db` |
| Agent runtime correctness, autonomy, and simplification | M002 — textual tool-call repair safety | closed | `plans/closure/agent-runtime-correctness-autonomy-simplification/002-status.md` | `86f8f43` |
| Agent runtime correctness, autonomy, and simplification | M001 — MCP authority, provenance, and tool-surface correctness | closed | `plans/closure/agent-runtime-correctness-autonomy-simplification/001-status.md` | `fb972426` |
| Post-audit correctness, simplification, and footprint | C002 — sandbox rights correction and strict closure | closed | `plans/closure/post-audit-correctness-simplification/010-sandbox-rights-correction-status.md` | `855de301`; hosted run `31425564638` |
| Post-audit correctness, simplification, and footprint | C001 — corrective PR integration | closed | `plans/closure/post-audit-correctness-simplification/009-corrective-status.md` | `8a556f05`; reconciled by C002 |
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

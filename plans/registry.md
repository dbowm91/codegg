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
| Agent runtime correctness, autonomy, and simplification | closed | `plans/subsystems/agent-runtime-correctness-autonomy-simplification-roadmap.md` | M009 closed | Hosted `verify` run `31515706555` passed on final candidate `c5154701`. |
| Post-audit correctness, simplification, and footprint | closed | `plans/subsystems/post-audit-correctness-simplification-corrective-closure-addendum.md` | C002 closed | C002 corrected the `/dev/null` Landlock path-rights defect; hosted `verify` run `31425564638` passed on the actual merge candidate. |
| Runtime safety, resource control, and footprint | conditionally closed | `plans/subsystems/runtime-safety-resource-footprint-roadmap.md` | C002 conditionally closed | Production implementation is merged. Only the previously named supported-Linux Landlock fixture evidence remains; it is independent of the new agent-runtime workstream. |

## Dependency-ready implementation plans

| Subsystem | Milestone | Status | Implementation plan | Dependencies |
|---|---|---|---|---|

## Active closure work

No active closure work remains for the agent-runtime correctness workstream; M009
is closed with the closure record below.

The previously completed post-audit corrective line remains closed. Historical control points are retained in:

- `plans/subsystems/post-audit-correctness-simplification-corrective-closure-addendum.md`
- `plans/closure/post-audit-correctness-simplification/009-corrective-status.md`
- `plans/closure/post-audit-correctness-simplification/010-sandbox-rights-correction-status.md`

Runtime-safety C002 remains conditionally closed only on its previously recorded supported-Linux Landlock fixture evidence in `plans/closure/runtime-safety-resource-footprint/010-status.md`. Do not create another runtime-safety milestone for that external evidence item.

## Blocked work

No registered implementation plans are currently blocked by this workstream.

## Execution order

The new workstream should execute correctness before broad harness cleanup:

1. M001, M002, M003, and M004 may proceed independently. Prefer M001/M002 early because they close execution-authority boundaries used by later recovery work.
2. M007 and M008 are closed; M009 is now ready and owns the final integration and closure evidence.
3. M005 becomes ready only after M001, M002, and M004 close. M003 is a soft dependency and should preferably be complete first to reduce incidental loop state.
4. M006 follows M005 so prompt/control consolidation targets the final recovery semantics rather than obsolete nudges.
5. M009 is the sole integration/closure milestone and is ready now that M001-M008 have accepted closure records.

Verification for this line remains minimal and change-specific: focused tests for each corrected invariant, `scripts/verify.sh quick` after coherent production milestones, and one broad final integration/hosted `verify` pass in M009. M007 measurements are manual diagnostics, not gates. M008 must reduce routine verification rather than add lanes, matrices, artifacts, scheduled audits, automatic publication, or release cadence.

## Workstream closure policy

The active agent-runtime correctness workstream closes only through M009 and `plans/closure/agent-runtime-correctness-autonomy-simplification/009-status.md`. M001-M008 require their own milestone closure records because M009 depends on accepted evidence rather than implementation claims.

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
| Agent runtime correctness, autonomy, and simplification | M009 — integration, documentation, and closure | closed | `plans/closure/agent-runtime-correctness-autonomy-simplification/009-status.md` | `c5154701`; hosted run `31515706555` |
| Agent runtime correctness, autonomy, and simplification | M007 — measured binary footprint and upstream dependency review | closed | `plans/closure/agent-runtime-correctness-autonomy-simplification/007-status.md` | `deb07a2` |
| Agent runtime correctness, autonomy, and simplification | M008 — routine CI and static-guard contraction | closed | `plans/closure/agent-runtime-correctness-autonomy-simplification/008-status.md` | `66326ad` |
| Agent runtime correctness, autonomy, and simplification | M006 — prompt compilation and control-policy consolidation | closed | `plans/closure/agent-runtime-correctness-autonomy-simplification/006-status.md` | `4cd004d` |
| Agent runtime correctness, autonomy, and simplification | M004 — turn identity, accounting, and lifecycle correctness | closed | `plans/closure/agent-runtime-correctness-autonomy-simplification/004-status.md` | `493fd59` |
| Agent runtime correctness, autonomy, and simplification | M005 — recovery and autonomy state machine | closed | `plans/closure/agent-runtime-correctness-autonomy-simplification/005-status.md` | `ddb495a` |
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

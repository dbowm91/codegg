# CodeGG Active Planning Registry

This file is the compact control surface for active interim planning. Detailed requirements and completed history remain in source roadmaps, implementation plans, `plans/closure/`, and Git history.

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
| Domain identity and compatibility | closed | `plans/subsystems/domain-identity-roadmap.md` | Milestone 4 closed | — |
| Runtime assets and harness interoperability | closed | `plans/subsystems/runtime-assets-roadmap.md` | Milestone 4 closed | — |
| Provider connections and Eggpool | closing | `plans/subsystems/provider-tool-dvr-independent-closure-ratification-addendum.md` | Milestone 007 conditionally closed | Provider/storage review passes; hosted verify `30681164263` fails on unrelated workspace Clippy dead-code errors in `crates/codegg-core/build.rs` |
| Project catalog and lazy discovery | closed | `plans/subsystems/project-catalog-roadmap.md` | Milestone 4 closed | — |
| Multi-project TUI and sessions | closed | `plans/subsystems/tui-project-sessions-roadmap.md` | Milestones 001–004 closed | — |
| Frontend-neutral session projections | closed | `plans/subsystems/session-projections-roadmap.md` | Milestone 012 closed | — |
| Agent runtime, model adaptation, and ACP | closed | `plans/subsystems/agent-runtime-model-adaptation-acp-corrective-closure-addendum.md` | M017 closed | — |
| Agent runtime correctness, autonomy, and simplification | closed | `plans/subsystems/agent-runtime-correctness-autonomy-simplification-corrective-closure-addendum.md` | M011 closed | Exact candidate `e3b671ad`; hosted run `31525206176` / job `93891703941` passed through Workspace tests. |
| Runtime consolidation, deletion, and footprint | closing | `plans/subsystems/runtime-consolidation-deletion-footprint-roadmap.md` | M007 conditionally closed | `plans/closure/runtime-consolidation-deletion-footprint/007-status.md`; hosted run `31710798729` and production-feature footprint evidence remain named conditions. |
| Programmatic tool execution and Tool Programs | closing | `plans/subsystems/provider-tool-dvr-independent-closure-ratification-addendum.md` | Milestone 019 ready | M018 fixture implementation is accepted and green; `018-status.md` remains provisional implementation evidence, and M019 owns independent strict review and isolation ratification |
| Development verification and release | active | `plans/subsystems/provider-tool-dvr-independent-closure-ratification-addendum.md` | Milestone 006 blocked | Final DVR closure requires strict Provider M007 and Tool Programs M019 records before independent DVR review may proceed |
| Runtime safety, resource control, and footprint | conditionally closed | `plans/subsystems/runtime-safety-resource-footprint-roadmap.md` | C002 conditionally closed | Only the previously recorded supported-Linux Landlock fixture evidence remains. |
| Post-audit correctness, simplification, and footprint | closed | `plans/subsystems/post-audit-correctness-simplification-corrective-closure-addendum.md` | C002 closed | Corrective `/dev/null` Landlock path-rights defect closed with hosted run `31425564638`. |

## Dependency-ready implementation plans

| Subsystem | Milestone | Status | Implementation plan | Dependencies |
|---|---|---|---|---|
| Runtime consolidation, deletion, and footprint | M006 — measured dependency and binary-footprint cleanup | ready | `plans/implementation/runtime-consolidation-deletion-footprint/006-measured-dependency-binary-cleanup.md` | M001–M005 closed; repeat measurements on consolidated tree. |
| Programmatic tool execution and Tool Programs | 019 — independent strict closure and evidence ratification | ready | `plans/implementation/tool-programs/019-independent-strict-closure-and-evidence-ratification.md` | M018 implementation landed; repeated-run and green full/hosted evidence are available for independent review |

## Active closure work

### Runtime consolidation, deletion, and footprint

This roadmap is a new cross-cutting consolidation workstream based on current repository evidence at baseline `bd9b3b61`; it does not reopen the closed Agent Runtime M011 or Agent Runtime/Model Adaptation/ACP M017 scopes. It consumes their canonical boundaries and removes migration residue around them.

Execution order:

1. M001–M005 are closed, including the M003 corrective physical extraction.
2. M006 is ready and must record measurements against the consolidated tree.
3. M007 starts only after M006 closes and owns the single broad integration/hosted closure pass.

Verification remains minimal and change-specific. This roadmap explicitly forbids new CI lanes/matrices, scheduled audits, coverage/benchmark/size gates, dependency bots, workflow-dispatch mechanisms, release automation, or fixed release cadence.

### Agent runtime correctness, autonomy, and simplification

M010 remains historical conditional evidence in `plans/closure/agent-runtime-correctness-autonomy-simplification/010-status.md`. Its structural recovery corrections remain accepted, but its strict disposition is superseded by M011 because later evidence changed the repository state:

- hosted `CI / verify` run `31521674076`, job `93879950640`, failed at Workspace Clippy on the obsolete empty `autonomy_bootstrap_is_explicitly_one_shot` test;
- ordinary native tool execution still had `Result<String, ToolError>` available but rendered failures to strings before recovery, so known `Permission`/`Timeout` status was not yet preserved through the authoritative path.

M011 was the sole controlling strict closure milestone for that workstream and is strictly closed by `plans/closure/agent-runtime-correctness-autonomy-simplification/011-status.md`. Its exact candidate `e3b671ad` passed hosted run `31525206176` / job `93891703941` through Workspace tests.

### Other active closure dependencies

- Agent runtime/model adaptation/ACP M017 is closed by `plans/closure/agent-runtime-model-adaptation-acp/017-status.md`.
- Tool Programs M019 remains ready and owns independent strict Tool Programs closure.
- Provider M007 remains conditionally closed pending its named hosted workspace-gate evidence.
- Development Verification and Release M006 remains blocked until Provider M007 and Tool Programs M019 are strictly closed.
- Runtime-safety C002 remains conditionally closed only on its previously recorded supported-Linux Landlock fixture evidence; do not create another runtime-safety milestone for that external evidence item.

## Blocked work

| Subsystem | Milestone | Blocker |
|---|---|---|
| Runtime consolidation, deletion, and footprint | M007 — integration, verification, and strict closure | conditionally closed | `plans/closure/runtime-consolidation-deletion-footprint/007-status.md`; hosted and production-feature evidence conditions remain. |
| Runtime consolidation, deletion, and footprint | M006 — measured dependency and binary-footprint cleanup | Ready after M003 corrective physical extraction; repeat the recorded audit. |
| Development verification and release | M006 | Strict Provider M007 and Tool Programs M019 closure records |

## Agent-runtime correctness execution order

1. M001-M009 remain historical predecessor work and are not reopened.
2. M010 remains conditionally closed historical corrective evidence; its bootstrap/dead-branch/continuation cleanup must not regress.
3. M011 owned the remaining stale hosted-Clippy test, typed tool-outcome propagation, and final exact hosted evidence; it is closed by `plans/closure/agent-runtime-correctness-autonomy-simplification/011-status.md`.
4. The exact final candidate passed focused/local verification and hosted run `31525206176` / job `93891703941` through Workspace tests.
5. The runtime-consolidation roadmap may simplify code around this closed boundary but must not rewrite M011 history or weaken its accepted invariants.

## Agent-runtime correctness closure policy

The agent-runtime correctness/autonomy/simplification workstream remains closed through M011 and `plans/closure/agent-runtime-correctness-autonomy-simplification/011-status.md`.

Historical control points remain:

- M005: `plans/closure/agent-runtime-correctness-autonomy-simplification/005-status.md`
- M009: `plans/closure/agent-runtime-correctness-autonomy-simplification/009-status.md`
- M010: `plans/closure/agent-runtime-correctness-autonomy-simplification/010-status.md`
- corrective addendum: `plans/subsystems/agent-runtime-correctness-autonomy-simplification-corrective-closure-addendum.md`

M010 must not be rewritten to conceal that its exact hosted evidence was unavailable at authorship and later became a failed hosted run. M011 must continue to cite the failed predecessor run and the final accepted green run.

Strict closure was accepted because all M011 acceptance criteria were met, no critical/high/medium unresolved finding remained in its scope, and hosted `CI / verify` run `31525206176` / job `93891703941` was green on the exact accepted candidate.

## Recently closed or conditionally closed control points

| Subsystem | Milestone | Status | Closure / controlling evidence |
|---|---|---|---|
| Runtime consolidation, deletion, and footprint | M001 — legacy background scheduler deletion | closed | `plans/closure/runtime-consolidation-deletion-footprint/001-status.md`; implementation commits `9594429`, `fcfed87` |
| Runtime consolidation, deletion, and footprint | M002 — structured outcome and recovery convergence | closed | `plans/closure/runtime-consolidation-deletion-footprint/002-status.md` |
| Runtime consolidation, deletion, and footprint | M003 — AgentLoop ownership decomposition | closed | `plans/closure/runtime-consolidation-deletion-footprint/003-status.md`; corrective physical extraction accepted |
| Runtime consolidation, deletion, and footprint | M004 — prompt/provider/history legacy deletion | closed | `plans/closure/runtime-consolidation-deletion-footprint/004-status.md`; implementation commit `0363d8f` |
| Runtime consolidation, deletion, and footprint | M005 — verification ratchet retirement and documentation contraction | closed | `plans/closure/runtime-consolidation-deletion-footprint/005-status.md` |
| Agent runtime correctness, autonomy, and simplification | M010 — recovery-state strict closure corrective pass | conditionally closed | `plans/closure/agent-runtime-correctness-autonomy-simplification/010-status.md`; structural correction retained; strict closure transferred to M011 after hosted run `31521674076` failed Clippy and typed-result review found incomplete propagation |
| Agent runtime correctness, autonomy, and simplification | M011 — typed tool outcome and hosted closure corrective pass | closed | `plans/closure/agent-runtime-correctness-autonomy-simplification/011-status.md`; exact candidate `e3b671ad`; hosted run `31525206176` / job `93891703941` passed through Workspace tests |
| Agent runtime correctness, autonomy, and simplification | M001-M008 | closed | Individual records under `plans/closure/agent-runtime-correctness-autonomy-simplification/` |
| Agent runtime, model adaptation, and ACP | M017 — corrective integration evidence and closure | closed | `plans/closure/agent-runtime-model-adaptation-acp/017-status.md` |
| Post-audit correctness, simplification, and footprint | C002 — sandbox rights correction and strict closure | closed | `plans/closure/post-audit-correctness-simplification/010-sandbox-rights-correction-status.md`; hosted run `31425564638` |
| Runtime safety, resource control, and footprint | C002 | conditionally closed | `plans/closure/runtime-safety-resource-footprint/010-status.md` |
| Provider connections and Eggpool | M007 | conditionally closed | `plans/closure/provider-connections/007-status.md` |
| Programmatic tool execution and Tool Programs | M018 | provisional/conditional implementation evidence | `plans/closure/tool-programs/018-status.md`; strict review owned by M019 |

Detailed predecessor history is intentionally not duplicated here. Use the source subsystem roadmaps and `plans/closure/` records for older milestones.

## Deferred unregistered product work

These remain outside active corrective handoff unless a concrete product priority or new evidence makes them ready:

- arbitrary non-UTF-8 command/protocol transport unless elevated by an explicit supported-platform contract;
- binary topology split or separate daemon/TUI packaging without new measured deployment need;
- replacing RustPython with a custom Tool Program parser;
- generalized HTTP/provider-client unification;
- broad Comrak/MSRV migration;
- broad Ratatui/TUI dependency migration beyond the bounded M006 maintenance evaluation unless a future dependency-maintenance priority explicitly activates it;
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

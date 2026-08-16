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
| Runtime consolidation, deletion, and footprint | closed | `plans/subsystems/runtime-consolidation-deletion-footprint-tui-closure-addendum.md` | M010 closed | M010 closure accepted; durable TUI schedule identity and labels are reconciled. |
| Programmatic tool execution and Tool Programs | closing | `plans/subsystems/provider-tool-dvr-independent-closure-ratification-addendum.md` | Milestone 019 ready | M018 fixture implementation is accepted and green; `018-status.md` remains provisional implementation evidence, and M019 owns independent strict review and isolation ratification |
| Development verification and release | active | `plans/subsystems/provider-tool-dvr-independent-closure-ratification-addendum.md` | Milestone 006 blocked | Final DVR closure requires strict Provider M007 and Tool Programs M019 records before independent DVR review may proceed |
| Runtime safety, resource control, and footprint | conditionally closed | `plans/subsystems/runtime-safety-resource-footprint-roadmap.md` | C002 conditionally closed | Only the previously recorded supported-Linux Landlock fixture evidence remains. |
| Post-audit correctness, simplification, and footprint | closed | `plans/subsystems/post-audit-correctness-simplification-daemon-lifecycle-corrective-addendum.md` | C003 closed | `plans/closure/post-audit-correctness-simplification/012-status.md`; C001/C002 remain historical closed evidence. |
| Search and eggsearch integration | closed | `plans/subsystems/search-eggsearch-integration-deep-research-corrective-addendum.md` | M004 closed | `plans/closure/search-eggsearch-integration/004-status.md`; M001–M003 remain historical accepted evidence |

## Dependency-ready implementation plans

| Subsystem | Milestone | Status | Implementation plan | Dependencies |
|---|---|---|---|---|
| Programmatic tool execution and Tool Programs | 019 — independent strict closure and evidence ratification | ready | `plans/implementation/tool-programs/019-independent-strict-closure-and-evidence-ratification.md` | M018 implementation landed; repeated-run and green full/hosted evidence are available for independent review |

## Closure work and dependencies

### Runtime consolidation, deletion, and footprint

M009 remains the historical architectural closure record: it converged the TUI on the durable `Schedule*` API, completed provider-turn physical ownership, closed M006 final-tree measurements, and obtained exact-candidate hosted verification. A later audit found one narrower supported-TUI contract defect that M009's regression evidence did not exercise.

Current controlling execution order:

1. Preserve M001–M009 implementation and closure history; do not reopen scheduler architecture, provider ownership, prompt/recovery, or footprint work.
2. M010 is the sole ready corrective handoff. It must make the short schedule token shown by `/tasks` and the schedule-created toast resolvable to exactly one full durable ID in the active workspace before deletion.
3. M010 must restore a meaningful `/tasks` prompt/label through the existing durable `ScheduleGet` record path, without another persistence source or public prefix-delete API.
4. M010 closure requires focused resolver/label tests plus one create -> list -> displayed-token -> delete -> list-absent regression through the durable client/daemon path.
5. After accepted `plans/closure/runtime-consolidation-deletion-footprint/010-status.md`, the TUI closure addendum may return to closed and M010 moves out of dependency-ready work.

M006 footprint measurements remain accepted and are not rerun for this TUI-only correction unless implementation unexpectedly changes dependencies, features, release profile, or topology.

Verification remains minimal and change-specific. Do not add CI lanes/matrices, scheduled audits, source scanners, coverage/benchmark/size gates, dependency bots, workflow-dispatch mechanisms, release automation, or fixed release cadence.

### Agent runtime correctness, autonomy, and simplification

M010 remains historical conditional evidence in `plans/closure/agent-runtime-correctness-autonomy-simplification/010-status.md`. Its structural recovery corrections remain accepted, but its strict disposition is superseded by M011 because later evidence changed the repository state:

- hosted `CI / verify` run `31521674076`, job `93879950640`, failed at Workspace Clippy on the obsolete empty `autonomy_bootstrap_is_explicitly_one_shot` test;
- ordinary native tool execution still had `Result<String, ToolError>` available but rendered failures to strings before recovery, so known `Permission`/`Timeout` status was not yet preserved through the authoritative path.

M011 was the sole controlling strict closure milestone for that workstream and is now strictly closed by `plans/closure/agent-runtime-correctness-autonomy-simplification/011-status.md`. Its exact candidate `e3b671ad` passed hosted run `31525206176` / job `93891703941` through Workspace tests.

### Post-audit daemon lifecycle corrective pass

C001/C002 remain accepted historical closure evidence. C003 is a new corrective control point created from later production-entrypoint evidence and must not rewrite those records.

C003 owns the smallest coherent production lifecycle correction:

1. restore production daemon catalog bootstrap/migration instead of project-local legacy-store authority;
2. make connect-or-start ownership/race semantics correct so an autostarted daemon survives its initiating frontend and concurrent clients converge on one lock winner;
3. make local socket handshake/readiness, peer-death propagation, reconnect disposition, and endpoint overrides finite and coherent;
4. make SIGTERM use graceful cancellation with bounded connection draining and owned runtime-artifact cleanup;
5. make daemon startup logs observable and drain local-MCP stderr so piped child output cannot deadlock the child.

C003 closure requires a real ordinary-startup smoke path, multi-process lifecycle regressions, focused transport/process tests, and `scripts/verify.sh quick`. It must not add a service manager, new CI topology, binary split, or scheduler/protocol redesign.

### Search and eggsearch integration

The 2026-08-15 audit found that CodeGG still selects eggsearch correctly by default, but specialized wrapper schemas had drifted from eggsearch 0.3.6 and direct Exa/research-provider clients bypassed the intended ownership boundary. M001–M003 corrected those issues and recorded wrapper-level current-process compatibility. A 2026-08-16 post-closure review then found a narrower deep-research consumer defect that the earlier verification did not exercise.

Current controlling execution order:

1. Preserve M001 request-contract repair and its accepted `plans/closure/search-eggsearch-integration/001-status.md` evidence.
2. Preserve M002 provider-ownership consolidation and its accepted `plans/closure/search-eggsearch-integration/002-status.md` evidence; do not reintroduce direct Exa/Tavily/Brave/SerpAPI/Kagi paths.
3. Preserve M003's structured MCP/search-backend implementation and real eggsearch 0.3.6 wrapper smoke as historical accepted evidence. Do not rewrite `plans/closure/search-eggsearch-integration/003-status.md` to conceal the later finding.
4. M004 is strictly closed by `plans/closure/search-eggsearch-integration/004-status.md`; it made `EggsearchSource` consume `dispatch_*_structured`, flatten current `research_search` `groups[*].results` into `SourceRecord`s, map every CodeGG `ResearchMode` to a supported upstream workflow disposition, use structured security evidence, and retain structured repo-search metadata through the `codesearch` compatibility alias.
5. M004 closure evidence includes focused current-shaped conversion/workflow tests, a fake-MCP consumer path through the research source boundary, truncation-vs-structured-value evidence, the focused `codesearch` assertion, and green `scripts/verify.sh quick`.
6. The corrective addendum and search/eggsearch subsystem are returned to closed; M001–M004 closure records remain authoritative without rewriting M003 history.
7. Verification remains deliberately light: no network CI lane, scheduled compatibility job, version matrix, source scanner, or release gate is added. The M003 real-process wrapper smoke remains valid and need not be repeated unless implementation evidence makes it necessary.

### Other active closure dependencies

- Agent runtime/model adaptation/ACP M017 is closed by `plans/closure/agent-runtime-model-adaptation-acp/017-status.md`.
- Tool Programs M019 remains ready and owns independent strict Tool Programs closure.
- Provider M007 remains conditionally closed pending its named hosted workspace-gate evidence; this is the independent Provider subsystem, not runtime-consolidation M007.
- Development Verification and Release M006 remains blocked until Provider M007 and Tool Programs M019 are strictly closed.
- Runtime-safety C002 remains conditionally closed only on its previously recorded supported-Linux Landlock fixture evidence; do not create another runtime-safety milestone for that external evidence item.

## Blocked work

| Subsystem | Milestone | Blocker |
|---|---|---|
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
| Runtime consolidation, deletion, and footprint | M001 — legacy background scheduler deletion | closed with corrective compatibility follow-up | `plans/closure/runtime-consolidation-deletion-footprint/001-status.md`; scheduler deletion remains accepted, M009 owns the discovered active-TUI compatibility regression. |
| Runtime consolidation, deletion, and footprint | M002 — structured outcome and recovery convergence | closed | `plans/closure/runtime-consolidation-deletion-footprint/002-status.md` |
| Runtime consolidation, deletion, and footprint | M003 — AgentLoop ownership decomposition | closed | `plans/closure/runtime-consolidation-deletion-footprint/003-status.md`; corrective context/tool/provider physical extraction accepted. |
| Runtime consolidation, deletion, and footprint | M004 — prompt/provider/history legacy deletion | closed | `plans/closure/runtime-consolidation-deletion-footprint/004-status.md`; implementation commit `0363d8f` |
| Runtime consolidation, deletion, and footprint | M005 — verification ratchet retirement and documentation contraction | closed | `plans/closure/runtime-consolidation-deletion-footprint/005-status.md` |
| Runtime consolidation, deletion, and footprint | M007 — integration evidence (historical provisional record) | archived/superseded | `plans/closure/runtime-consolidation-deletion-footprint/007-status.md`; earlier provisional evidence is retained by history and superseded by the strict record below. |
| Runtime consolidation, deletion, and footprint | M006 — measured dependency and binary-footprint cleanup | closed | `plans/closure/runtime-consolidation-deletion-footprint/006-status.md`; final candidate `c8c31d90`, default 54,347,840 bytes, production features 63,566,624 bytes |
| Runtime consolidation, deletion, and footprint | M007 — integration, verification, and strict closure | closed | `plans/closure/runtime-consolidation-deletion-footprint/007-status.md`; exact hosted run `31724978736` / job `94530985774` |
| Runtime consolidation, deletion, and footprint | M009 — architectural corrective closure (historical) | closed; current TUI disposition superseded by M010 | `plans/closure/runtime-consolidation-deletion-footprint/009-status.md`; later audit found the short-ID deletion and missing-label defects now owned by M010. |
| Runtime consolidation, deletion, and footprint | M010 — TUI durable schedule identity and label closure | closed | `plans/closure/runtime-consolidation-deletion-footprint/010-status.md`; implementation `58dd05de`; no registered future plan was unblocked. |
| Agent runtime correctness, autonomy, and simplification | M010 — recovery-state strict closure corrective pass | conditionally closed | `plans/closure/agent-runtime-correctness-autonomy-simplification/010-status.md`; structural correction retained; strict closure transferred to M011 after hosted run `31521674076` failed Clippy and typed-result review found incomplete propagation |
| Agent runtime correctness, autonomy, and simplification | M011 — typed tool outcome and hosted closure corrective pass | closed | `plans/closure/agent-runtime-correctness-autonomy-simplification/011-status.md`; exact candidate `e3b671ad`; hosted run `31525206176` / job `93891703941` passed through Workspace tests |
| Agent runtime correctness, autonomy, and simplification | M001-M008 | closed | Individual records under `plans/closure/agent-runtime-correctness-autonomy-simplification/` |
| Agent runtime, model adaptation, and ACP | M017 — corrective integration evidence and closure | closed | `plans/closure/agent-runtime-model-adaptation-acp/017-status.md` |
| Post-audit correctness, simplification, and footprint | C002 — sandbox rights correction and strict closure | closed | `plans/closure/post-audit-correctness-simplification/010-sandbox-rights-correction-status.md`; hosted run `31425564638` |
| Post-audit correctness, simplification, and footprint | C003 — daemon startup, shutdown, and process-lifecycle corrective pass | closed | `plans/closure/post-audit-correctness-simplification/012-status.md`; implementation `0bb7d5b`; no registered future plan was unblocked. |
| Runtime safety, resource control, and footprint | C002 | conditionally closed | `plans/closure/runtime-safety-resource-footprint/010-status.md` |
| Provider connections and Eggpool | M007 | conditionally closed | `plans/closure/provider-connections/007-status.md` |
| Programmatic tool execution and Tool Programs | M018 | provisional/conditional implementation evidence | `plans/closure/tool-programs/018-status.md`; strict review owned by M019 |
| Search and eggsearch integration | M001 — current eggsearch request-contract repair | closed | `plans/closure/search-eggsearch-integration/001-status.md`; implementation `acb6ba8`; M002 unblocked |
| Search and eggsearch integration | M002 — external search ownership consolidation | closed | `plans/closure/search-eggsearch-integration/002-status.md`; implementation `e46f97d2`; M003 moved to ready |
| Search and eggsearch integration | M003 — structured contract and compatibility closure | historical closed evidence; current strict disposition superseded by M004 | `plans/closure/search-eggsearch-integration/003-status.md`; implementation `89dbac7`; later audit found the deep-research consumer/workflow gap now owned by M004 |
| Search and eggsearch integration | M004 — deep-research structured-consumption corrective pass | closed | `plans/closure/search-eggsearch-integration/004-status.md`; implementation `6f1fa20a`; no unrelated registered future plan was unblocked |

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

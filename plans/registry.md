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
| Provider connections and Eggpool | closed | `plans/subsystems/provider-tool-dvr-independent-closure-ratification-addendum.md` | Milestone 007 closed | — |
| Project catalog and lazy discovery | closed | `plans/subsystems/project-catalog-roadmap.md` | Milestone 4 closed | — |
| Multi-project TUI and sessions | closed | `plans/subsystems/tui-project-sessions-roadmap.md` | Milestones 001–004 closed | — |
| Frontend-neutral session projections | closed | `plans/subsystems/session-projections-roadmap.md` | Milestone 012 closed | — |
| Agent runtime, model adaptation, and ACP | closed | `plans/subsystems/agent-runtime-model-adaptation-acp-corrective-closure-addendum.md` | M017 closed | — |
| Agent runtime correctness, autonomy, and simplification | closed | `plans/subsystems/agent-runtime-correctness-autonomy-simplification-corrective-closure-addendum.md` | M011 closed | Exact candidate `e3b671ad`; hosted run `31525206176` / job `93891703941` passed through Workspace tests. |
| Agent runs, async delegation, and worktree concurrency | active | `plans/subsystems/agent-run-worktree-concurrency-roadmap.md` | M006 active | M001-M005 closed; M006 closure evidence is being gathered. |
| Runtime consolidation, deletion, and footprint | closed | `plans/subsystems/runtime-consolidation-deletion-footprint-tui-closure-addendum.md` | M010 closed | M010 closure accepted; durable TUI schedule identity and labels are reconciled. |
| Programmatic tool execution and Tool Programs | closed | `plans/subsystems/tool-programs-roadmap.md` | M019 strict closure + M020 corrective disposition accepted | — |
| Development verification and release | closed | `plans/subsystems/development-verification-release-final-evidence-closure-addendum.md` | Milestone 007 closed | — |
| Runtime safety, resource control, and footprint | conditionally closed | `plans/subsystems/runtime-safety-resource-footprint-roadmap.md` | C002 conditionally closed | Only the previously recorded supported-Linux Landlock fixture evidence remains. |
| Post-audit correctness, simplification, and footprint | closed | `plans/subsystems/post-audit-correctness-simplification-daemon-lifecycle-corrective-addendum.md` | C003 closed | `plans/closure/post-audit-correctness-simplification/012-status.md`; C001/C002 remain historical closed evidence. |
| Search and eggsearch integration | closed | `plans/subsystems/search-eggsearch-integration-roadmap.md` | M005 closed | — |

## Dependency-ready implementation plans

| Subsystem | Milestone | Plan | Why ready |
|---|---|---|---|
| Agent runs, async delegation, and worktree concurrency | M006 — projection, compatibility, and closure | `plans/implementation/agent-run-worktree-concurrency/006-projection-compatibility-and-closure.md` | M001-M005 closure records accepted; implementation and strict closure evidence are in progress. |

## Closure work and dependencies

All previously active closure lines are closed. M001-M005 are closed by their linked records; M006 is ready after the M005 closure audit.

Historical closed control points remain:

- Provider M007 strict closure: `plans/closure/provider-connections/007-status.md` (hosted run `30931979689`, job `92084050226`, revision `c85980e2`). The earlier conditional disposition and hosted Clippy failure (`30681164263`) are preserved as historical evidence inside the record.
- Tool Programs M019 independent strict review: `plans/closure/tool-programs/019-status.md`. `018-status.md` remains provisional implementation-authored historical evidence.
- Tool Programs M020 corrective disposition (child-artifact recovery): `plans/closure/tool-programs/020-status.md`.
- DVR M007 minimal verification contract and final closure: `plans/closure/development-verification-release/007-status.md`.
- Runtime consolidation M010, agent-runtime M011/M017, post-audit C003, and search M005 remain closed per their linked records below.

Verification remains deliberately light: the new workstream must use focused concurrency/restart/security tests plus the repository’s existing quick broad verification posture. No new CI lanes, scanners, coverage/benchmark/size gates, dependency bots, workflow-dispatch mechanisms, release automation, or fixed release cadence are added.

## Blocked work

The later agent-run/worktree milestones are intentionally dependency-gated rather than independently executable:

| Milestone | Plan | Blocker |
|---|---|---|
| None | — | No registered agent-run/worktree plan remains blocked after M005. |

These are planning dependencies, not external blockers. No unrelated previously closed subsystem is reopened.

## Agent-runtime correctness execution order

1. M001-M009 remain historical predecessor work and are not reopened.
2. M010 remains conditionally closed historical corrective evidence; its bootstrap/dead-branch/continuation cleanup must not regress.
3. M011 owned the remaining stale hosted-Clippy test, typed tool-outcome propagation, and final exact hosted evidence; it is closed by `plans/closure/agent-runtime-correctness-autonomy-simplification/011-status.md`.
4. The exact final candidate passed focused/local verification and hosted run `31525206176` / job `93891703941` through Workspace tests.
5. The new agent-run/worktree roadmap consumes this closed runtime as a dependency and must not rewrite M011 history or weaken its accepted recovery/tool-outcome invariants.

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
| Agent runs, async delegation, and worktree concurrency | M002 — run mailbox, journal, and async control | closed | `plans/closure/agent-run-worktree-concurrency/002-status.md`; implementation commit `36e19e6`; M003 was subsequently implemented and closed, unblocking M004. |
| Agent runs, async delegation, and worktree concurrency | M003 — durable worktree service and leases | closed | `plans/closure/agent-run-worktree-concurrency/003-status.md`; implementation commit `0f3d75bf`; its accepted dependency audit enabled M004. |
| Agent runs, async delegation, and worktree concurrency | M004 — isolated mutation and structured results | closed | `plans/closure/agent-run-worktree-concurrency/004-status.md`; implementation commit `37b9cc9c`; M005 was subsequently moved to ready. |
| Agent runs, async delegation, and worktree concurrency | M005 — run groups and background joins | closed | `plans/closure/agent-run-worktree-concurrency/005-status.md`; bounded durable group coordination, task-tool fan-out/control, group notifications, and restart-safe state. M006 moved to ready. |
| Agent runtime, model adaptation, and ACP | M017 — corrective integration evidence and closure | closed | `plans/closure/agent-runtime-model-adaptation-acp/017-status.md` |
| Post-audit correctness, simplification, and footprint | C002 — sandbox rights correction and strict closure | closed | `plans/closure/post-audit-correctness-simplification/010-sandbox-rights-correction-status.md`; hosted run `31425564638` |
| Post-audit correctness, simplification, and footprint | C003 — daemon startup, shutdown, and process-lifecycle corrective pass | closed | `plans/closure/post-audit-correctness-simplification/012-status.md`; implementation `0bb7d5b`; no registered future plan was unblocked. |
| Runtime safety, resource control, and footprint | C002 | conditionally closed | `plans/closure/runtime-safety-resource-footprint/010-status.md` |
| Provider connections and Eggpool | M007 — conditional disposition (historical) | superseded by strict closure | `plans/closure/provider-connections/007-status.md`; the record's historical sections preserve the earlier conditional result and hosted Clippy failure `30681164263`; see the strict row below |
| Programmatic tool execution and Tool Programs | M018 — runtime fixture correction (historical) | provisional implementation evidence retained; strict disposition owned by M019 | `plans/closure/tool-programs/018-status.md`; see the M019/M020 rows below |
| Search and eggsearch integration | M001 — current eggsearch request-contract repair | closed | `plans/closure/search-eggsearch-integration/001-status.md`; implementation `acb6ba8`; M002 unblocked |
| Search and eggsearch integration | M002 — external search ownership consolidation | closed | `plans/closure/search-eggsearch-integration/002-status.md`; implementation `e46f97d2`; M003 moved to ready |
| Search and eggsearch integration | M003 — structured contract and compatibility closure | historical closed evidence; current strict disposition superseded by later corrective milestones | `plans/closure/search-eggsearch-integration/003-status.md`; implementation `89dbac7`; M004 corrected the deep-research consumer gap |
| Search and eggsearch integration | M004 — deep-research structured-consumption corrective pass | historical closed implementation evidence; current strict disposition superseded by M005 | `plans/closure/search-eggsearch-integration/004-status.md`; implementation `6f1fa20a`; exact hosted run `31930352527` / job `95124064959` later failed on M004 Clippy and M005 owns remaining SourceCard/workflow fidelity |
| Search and eggsearch integration | M005 — hosted closure and SourceCard fidelity corrective pass | closed | `plans/closure/search-eggsearch-integration/005-status.md`; implementation/final candidate `75ccc70e`; hosted run `32047863303` / job `95439829669` passed through Workspace tests |
| Provider connections and Eggpool | M007 — independent closure ratification and governance reconciliation | closed (strict) | `plans/closure/provider-connections/007-status.md`; accepted revision `c85980e2`; shared hosted run `30931979689` / job `92084050226` passed on attempt 3; earlier conditional record retained as historical evidence |
| Programmatic tool execution and Tool Programs | M019 — independent strict closure and evidence ratification | closed | `plans/closure/tool-programs/019-status.md`; accepted revision `c85980e2`; shared hosted run `30931979689` / job `92084050226` |
| Programmatic tool execution and Tool Programs | M020 — canonical child-artifact recovery corrective closure | closed | `plans/closure/tool-programs/020-status.md`; implementation `c85980e2`; covered by the same green hosted run |
| Development verification and release | M007 — minimal verification contract and final closure | closed | `plans/closure/development-verification-release/007-status.md`; accepted revision `c85980e2`; boundary guard fail-open correction; no registered plan was left blocked |

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

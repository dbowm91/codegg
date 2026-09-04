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
| Provider connections and Eggpool | closed | `plans/subsystems/provider-direct-call-session-context-corrective-addendum.md` | Milestone 009 closed | Direct production provider callers now receive owning session/run context; M008 transport/header behavior remains preserved. |
| Project catalog and lazy discovery | closed | `plans/subsystems/project-catalog-roadmap.md` | Milestone 4 closed | — |
| Multi-project TUI and sessions | closed | `plans/subsystems/tui-project-sessions-roadmap.md` | Milestones 001–004 closed | — |
| Frontend-neutral session projections | closed | `plans/subsystems/session-projections-roadmap.md` | Milestone 012 closed | — |
| Agent runtime, model adaptation, and ACP | closed | `plans/subsystems/agent-runtime-model-adaptation-acp-corrective-closure-addendum.md` | M017 closed | — |
| Agent runtime correctness, autonomy, and simplification | closed | `plans/subsystems/agent-runtime-correctness-autonomy-simplification-corrective-closure-addendum.md` | M011 closed | Exact candidate `e3b671ad`; hosted run `31525206176` / job `93891703941` passed through Workspace tests. |
| Agent runtime — goal verification corrective follow-up | closed | `plans/subsystems/agent-runtime-goal-verification-corrective-addendum.md` | M013 closed | Exact-goal provenance, conservative criteria, and cross-goal evidence isolation accepted in `plans/closure/agent-runtime-correctness-autonomy-simplification/013-status.md`. |
| Agent runs, async delegation, and worktree concurrency | closed | `plans/subsystems/agent-run-worktree-concurrency-final-corrective-closure-addendum.md` | M009 closed | Root-turn completion, invocation scope, group-terminal projection, and exact-head CI corrections accepted; hosted run `33588719613` / job `100118138199` passed through Workspace tests. |
| Agent convergence and independent verification | closed | `plans/subsystems/agent-convergence-roadmap.md` | M003 closed | `plans/closure/agent-convergence/003-status.md`; bounded repair/replan, explicit commit chaining, conservative model gating, and projection closure accepted. |
| Memory-to-skill promotion | active | `plans/subsystems/memory-skill-promotion-hosted-verification-corrective-addendum.md` | M005 active; M004 corrective pass required | M004 fixed the two M001-owned habit lock opens, but hosted run `33836217483` / job `100909174354` on `7ef387aa` exposed six older M002/M003 publication/proposal Clippy findings; M005 owns those findings. |
| Runtime consolidation, deletion, and footprint | closed | `plans/subsystems/runtime-consolidation-deletion-footprint-tui-closure-addendum.md` | M010 closed | M010 closure accepted; durable TUI schedule identity and labels are reconciled. |
| Programmatic tool execution and Tool Programs | closed | `plans/subsystems/tool-programs-roadmap.md` | M019 strict closure + M020 corrective disposition accepted | — |
| Development verification and release | active | `plans/subsystems/development-verification-release-final-evidence-closure-addendum.md` | M008/M009 active; M007 remains closed | M008 owns the pre-existing `items_after_test_module` correction; M009 owns stale nine-agent test expectations exposed when hosted tests resumed. |
| Runtime safety, resource control, and footprint | conditionally closed | `plans/subsystems/runtime-safety-resource-footprint-roadmap.md` | C002 conditionally closed | Only the previously recorded supported-Linux Landlock fixture evidence remains. |
| Runtime safety — checked edit-history corrective follow-up | closed | `plans/subsystems/runtime-safety-edit-history-corrective-addendum.md` | M013 closed | `plans/closure/runtime-safety-resource-footprint/013-status.md`; exact candidate `f314c38e` passed hosted `CI / verify` run `33712437859` / job `100514597927`. |
| Post-audit correctness, simplification, and footprint | closed | `plans/subsystems/post-audit-correctness-simplification-daemon-lifecycle-corrective-addendum.md` | C003 closed | `plans/closure/post-audit-correctness-simplification/012-status.md`; C001/C002 remain historical closed evidence. |
| Search and eggsearch integration | closed | `plans/subsystems/search-eggsearch-integration-roadmap.md` | M005 closed | — |

## Dependency-ready implementation plans

| Subsystem | Milestone | Plan | Why ready |
|---|---|---|---|

## Newly registered feature execution order

1. Agent-convergence M001–M003 remain closed under their accepted closure records.
2. Memory-to-skill M001–M003 remain immutable historical closure evidence, but the current strict subsystem disposition is reopened under M004 because exact `main` head `4ea4eaa` failed hosted Workspace Clippy on M001-owned code.
3. Memory-to-skill M004 implementation is complete but its strict closure is corrective-pass-required because exact hosted CI exposed six M002/M003 publication/proposal findings outside M004's bounded ownership.
4. Memory-to-skill M005 is the active corrective milestone. It owns only the six reported publication/proposal Clippy findings and exact-head hosted closure; it must not broaden into habit, parser, asset-refresh, or generic filesystem redesign.
5. Development verification M008 and M009 are separate hosted-verification corrective milestones; M009 owns only stale built-in-agent test expectations and must not alter agent assets or runtime behavior.
5. Convergence must continue to compose existing `AgentRun`, run-group, run-control, `WorktreeService`, structured run results, and host goal verification. It must not extend the legacy file-backed team inbox/outbox path or create a second scheduler.
6. Habit promotion must retain structural privacy bounds: no raw shell/tool output/arguments in automatic habit fingerprints, no automatic model drafting, and no skill publication without explicit user approval.
7. No new ADR is required because scheduler/authorization/foreign-asset/storage ownership remain unchanged. If M005 discovers a need to change those boundaries, work stops for a separately registered follow-up rather than widening the milestone.
8. M004/M005 verification remains deliberately narrow: owning habit/memory/promotion tests, the exact workspace/all-target Clippy command, `scripts/verify.sh quick`, and the existing hosted `CI / verify` lane on the exact final candidate. No new CI lane, benchmark gate, scanner, or release automation is introduced.

## Closure work and dependencies

- Memory-to-skill M005 is active; its production correction and focused
  evidence are being prepared for exact-head hosted closure.
- Development verification M008 is active; it owns the separate hosted Clippy
  ordering correction exposed by the M005 candidate.

Agent-run/worktree M001–M008 remain historical closure records and MUST NOT be rewritten to conceal the accepted implementation history. M006 was superseded by the M007/M008 corrective work; the later M008 strict subsystem disposition was superseded by `plans/subsystems/agent-run-worktree-concurrency-final-corrective-closure-addendum.md` after the post-M008 production-path and exact-head CI audit. Provider M009 is now closed through `plans/closure/provider-connections/009-status.md`.

Historical closed control points remain:

- Agent-run/worktree M006 strict closure (historical/superseded disposition): `plans/closure/agent-run-worktree-concurrency/006-status.md` (reviewed implementation `7bc39c28`). Its evidence remains preserved; M007–M009 own later corrective disposition.
- Agent-run/worktree M008 strict closure (historical/superseded disposition): `plans/closure/agent-run-worktree-concurrency/008-status.md` (implementation `5ced31bf`). Its local evidence remains preserved, including the later-observed exact-head hosted Clippy failure; M009 owns the accepted final closure.
- Provider M007 strict closure: `plans/closure/provider-connections/007-status.md` (hosted run `30931979689`, job `92084050226`, revision `c85980e2`). The earlier conditional disposition and hosted Clippy failure (`30681164263`) are preserved as historical evidence inside the record.
- Provider M008 strict closure (historical for accepted transport scope): `plans/closure/provider-connections/008-status.md` (implementation `328c26cb`). Its typed request-context, OpenCode Go affinity policy, and static-header transport remain accepted; M009 owns the later-discovered incomplete classification of direct production provider callers.
- Provider M009 direct-call session-context corrective closure: `plans/closure/provider-connections/009-status.md`.
- Tool Programs M019 independent strict review: `plans/closure/tool-programs/019-status.md`. `018-status.md` remains provisional implementation-authored historical evidence.
- Tool Programs M020 corrective disposition (child-artifact recovery): `plans/closure/tool-programs/020-status.md`.
- DVR M007 minimal verification contract and final closure: `plans/closure/development-verification-release/007-status.md`.
- Runtime consolidation M010, agent-runtime M011/M017, post-audit C003, and search M005 remain closed per their linked records below.
- Runtime-assets M005/M006, goal-verification M012, and runtime-safety M011/M012 remain immutable historical closure evidence. Their current strict subsystem dispositions are superseded only by the corrective milestones M007/M013/M013; goal-verification M013 is now closed.
- Memory-to-skill M001–M003 closure records remain immutable historical evidence. M004 owns the later exact-head hosted Clippy failure in M001-introduced `memory/habit.rs`; M005 owns the six M002/M003 publication/proposal findings exposed by the M004 candidate; the final subsystem closure disposition remains open until M005.

Verification remains deliberately light: Provider M009 was accepted with focused direct-call request-context tests, retained M008 provider/header regression tests, and the repository's existing `scripts/verify.sh quick` posture. Existing historical closures retain their recorded evidence requirements. Memory-to-skill M004 adds no new verification framework; it uses the already-existing workspace Clippy and hosted CI lane because that is the exact failing evidence. No new CI lanes, scanners, coverage/benchmark/size gates, dependency bots, workflow-dispatch mechanisms, release automation, or fixed release cadence are added.

## Blocked work

- The historical supported-Linux Landlock evidence condition remains unchanged under the existing runtime-safety conditional closure and does not block M004.
- Memory-to-skill M005 is in progress after M004's strict closure was blocked by the six publication/proposal findings. M001–M004 historical records remain unchanged.
- Development verification M008 is in progress for the separate hosted Clippy `items_after_test_module` finding in `src/tool/review.rs`; it is not part of memory-to-skill M005 scope.
- Development verification M009 is in progress for stale nine-agent expectations in agent tests revealed after M008; it is not part of memory-to-skill M005 scope.

No newly registered corrective plan is hard-blocked. Provider M009 is closed; runtime-safety M013, runtime-assets M007, goal-verification M013, and Provider M008 are closed historical control points.

No new work is registered for browser-specific security, generic hook-taxonomy expansion, duplicate plugin/MCP runtimes, or opportunistic scheduling; the repository audit found those areas already owned by existing systems or insufficiently justified.

## New corrective execution order

1. Memory-to-skill M004 corrects the two M001-owned advisory lock-file opens so their non-truncating `OpenOptions` semantics are explicit; lint suppression, CI weakening, and toolchain downgrade are forbidden substitutes.
2. The failed predecessor evidence is exact head `4ea4eaa000ecf65b0e70ed7278cf071a57cf2197`, hosted run `33813852632`, job `100841494152`; historical M001/M003 closure records remain unchanged.
3. M004's hosted candidate `7ef387aa` fixed those two findings but exposed six older M002/M003 publication/proposal Clippy findings; M005 owns their behavior-preserving correction and final hosted closure.
4. After `plans/closure/memory-skill-promotion/005-status.md` is accepted, the registry may return memory-to-skill promotion to `closed at M005`; no downstream plan is currently blocked on M004.
5. Provider M009 is closed and preserves the accepted M008 transport implementation while correcting direct provider callers that dropped required session/run context.
6. Research model-backed phases reuse one stable research-run affinity value across extraction, claim, and verification calls.
7. Agent-invoked ReviewTool and CommitTool consume the existing `ToolExecutionContext.session_id`; standalone invocation uses one invocation-scoped identity when a provider request is made.
8. Async LLM compaction and every remaining production direct `Provider::stream()` call have an explicit reachability/identity disposition.
9. Focused direct-call propagation tests, retained M008 header tests, and `scripts/verify.sh quick` passed; prior closure records remain immutable historical evidence.

## Agent-runtime correctness execution order

1. M001-M009 remain historical predecessor work and are not reopened.
2. M010 remains conditionally closed historical corrective evidence; its bootstrap/dead-branch/continuation cleanup must not regress.
3. M011 owned the remaining stale hosted-Clippy test, typed tool-outcome propagation, and final exact hosted evidence; it is closed by `plans/closure/agent-runtime-correctness-autonomy-simplification/011-status.md`.
4. The exact final M011 candidate passed focused/local verification and hosted run `31525206176` / job `93891703941` through Workspace tests.
5. Agent-run/worktree corrective work consumes this closed runtime as a dependency and must not rewrite M011 history or weaken its accepted recovery/tool-outcome invariants.
6. Goal-verification M012 remains historical closed evidence that removed direct model-owned completion.
7. Goal-verification M013 was the corrective owner for exact goal evidence provenance, claimed-test scope, and deterministic criterion semantics; its strict closure is recorded below and it did not reopen unrelated recovery/prompt/tool-authority work.
8. No later registered plan names goal-verification M013 as a blocking dependency, so this closure unblocks no future plan.

## Agent-runtime correctness closure policy

The historical agent-runtime correctness/autonomy/simplification workstream remains closed through M011 and `plans/closure/agent-runtime-correctness-autonomy-simplification/011-status.md`. Goal-verification M012 remains a separate historical closure, and M013 is now closed under `plans/subsystems/agent-runtime-goal-verification-corrective-addendum.md`.

Historical control points remain:

- M005: `plans/closure/agent-runtime-correctness-autonomy-simplification/005-status.md`
- M009: `plans/closure/agent-runtime-correctness-autonomy-simplification/009-status.md`
- M010: `plans/closure/agent-runtime-correctness-autonomy-simplification/010-status.md`
- M011: `plans/closure/agent-runtime-correctness-autonomy-simplification/011-status.md`
- goal-verification M012: `plans/closure/agent-runtime-correctness-autonomy-simplification/012-status.md`
- corrective addendum: `plans/subsystems/agent-runtime-correctness-autonomy-simplification-corrective-closure-addendum.md`

M010 must not be rewritten to conceal that its exact hosted evidence was unavailable at authorship and later became a failed hosted run. M011 must continue to cite the failed predecessor run and the final accepted green run. Goal M012 likewise remains unchanged; M013 records and closes the later provenance/criterion findings instead of altering M012 history.

Strict M011 closure was accepted because all M011 acceptance criteria were met, no critical/high/medium unresolved finding remained in its scope, and hosted `CI / verify` run `31525206176` / job `93891703941` was green on the exact accepted candidate.

## Recently closed or conditionally closed control points

| Subsystem | Milestone | Status | Closure / controlling evidence |
|---|---|---|---|
| Runtime consolidation, deletion, and footprint | M001 — legacy background scheduler deletion | closed with corrective compatibility follow-up | `plans/closure/runtime-consolidation-deletion-footprint/001-status.md`; scheduler deletion remains accepted, M009 owns the discovered active-TUI compatibility regression. |
| Runtime consolidation, deletion, and footprint | M002 — structured outcome and recovery convergence | closed | `plans/closure/runtime-consolidation-deletion-footprint/002-status.md` |
| Runtime consolidation, deletion, and footprint | M003 — AgentLoop ownership decomposition | closed | `plans/closure/runtime-consolidation-deletion-footprint/003-status.md`; corrective context/tool/provider physical extraction accepted. |
| Runtime consolidation, deletion, and footprint | M004 — prompt/provider/history legacy deletion | closed | `plans/closure/runtime-consolidation-deletion-footprint/004-status.md`; implementation `0363d8f` |
| Runtime consolidation, deletion, and footprint | M005 — verification ratchet retirement and documentation contraction | closed | `plans/closure/runtime-consolidation-deletion-footprint/005-status.md` |
| Runtime consolidation, deletion, and footprint | M007 — integration evidence (historical provisional record) | archived/superseded | `plans/closure/runtime-consolidation-deletion-footprint/007-status.md`; earlier provisional evidence is retained by history and superseded by the strict record below. |
| Runtime consolidation, deletion, and footprint | M006 — measured dependency and binary-footprint cleanup | closed | `plans/closure/runtime-consolidation-deletion-footprint/006-status.md`; final candidate `c8c31d90`, default 54,347,840 bytes, production features 63,566,624 bytes |
| Runtime consolidation, deletion, and footprint | M007 — integration, verification, and strict closure | closed | `plans/closure/runtime-consolidation-deletion-footprint/007-status.md`; exact hosted run `31724978736` / job `94530985774` |
| Runtime consolidation, deletion, and footprint | M009 — architectural corrective closure (historical) | closed; current TUI disposition superseded by M010 | `plans/closure/runtime-consolidation-deletion-footprint/009-status.md`; later audit found the short-ID deletion and missing-label defects now owned by M010. |
| Runtime consolidation, deletion, and footprint | M010 — TUI durable schedule identity and label closure | closed | `plans/closure/runtime-consolidation-deletion-footprint/010-status.md`; implementation `58dd05de`; no registered future plan was unblocked. |
| Runtime assets and harness interoperability | M005 — durable context-aware plugin activation | closed | `plans/closure/runtime-assets/005-status.md`; durable scoped activation and immutable context resolution; M006 moved to ready. |
| Runtime assets — plugin declarative contributions | M006 — passive asset and MCP contribution bridge | closed historical evidence; current compatibility disposition owned by M007 | `plans/closure/runtime-assets/006-status.md`; implementation `35cf6f5`; M007 owns the later-discovered `stdio`/`http` transport alias mismatch. |
| Runtime assets — plugin declarative contributions corrective follow-up | M007 — plugin MCP transport alias corrective pass | closed | `plans/closure/runtime-assets/007-status.md`; implementation `eb9c4d9`; no registered future plan was unblocked. |
| Agent runtime correctness, autonomy, and simplification | M010 — recovery-state strict closure corrective pass | conditionally closed | `plans/closure/agent-runtime-correctness-autonomy-simplification/010-status.md`; structural correction retained; strict closure transferred to M011 after hosted run `31521674076` failed Clippy and typed-result review found incomplete propagation |
| Agent runtime correctness, autonomy, and simplification | M011 — typed tool outcome and hosted closure corrective pass | closed | `plans/closure/agent-runtime-correctness-autonomy-simplification/011-status.md`; exact candidate `e3b671ad`; hosted run `31525206176` / job `93891703941` passed through Workspace tests |
| Agent runtime — host-owned goal verification | M012 — host-owned completion verification | closed historical evidence; corrective disposition accepted by M013 | `plans/closure/agent-runtime-correctness-autonomy-simplification/012-status.md`; implementations `25b85b7c`, `004f136c`; M013 owns the later exact-goal provenance and criterion-classification findings. |
| Agent runtime — goal verification | M013 — goal evidence provenance and criterion corrective pass | closed | `plans/closure/agent-runtime-correctness-autonomy-simplification/013-status.md`; exact-goal durable provenance and conservative criterion semantics accepted; no future registered plan was unblocked. |
| Agent convergence and independent verification | M002 — independent verifier and explicit owner decision | closed | `plans/closure/agent-convergence/002-status.md`; implementation `28008ddd`; M003 moved to ready. |
| Agent convergence and independent verification | M003 — bounded repair/replan and model gating | closed | `plans/closure/agent-convergence/003-status.md`; implementation `33ec0376`; no future registered plan was unblocked. |
| Agent runtime correctness, autonomy, and simplification | M001-M008 | closed | Individual records under `plans/closure/agent-runtime-correctness-autonomy-simplification/` |
| Agent runs, async delegation, and worktree concurrency | M002 — run mailbox, journal, and async control | closed historical evidence | `plans/closure/agent-run-worktree-concurrency/002-status.md`; implementation commit `36e19e6`; authorization/call-identity composition is now owned by M007–M009. |
| Agent runs, async delegation, and worktree concurrency | M003 — durable worktree service and leases | closed historical evidence | `plans/closure/agent-run-worktree-concurrency/003-status.md`; implementation commit `0f3d75bf`; nested context/base composition remains accepted after M007. |
| Agent runs, async delegation, and worktree concurrency | M004 — isolated mutation and structured results | closed historical evidence | `plans/closure/agent-run-worktree-concurrency/004-status.md`; implementation commit `37b9cc9c`; core isolation/result machinery remains accepted. |
| Agent runs, async delegation, and worktree concurrency | M005 — run groups and background joins | closed historical evidence; capability disposition superseded | `plans/closure/agent-run-worktree-concurrency/005-status.md`; group service/store landed; M007 fixed owner reachability and M009 owns final completion delivery/projection composition. |
| Agent runs, async delegation, and worktree concurrency | M006 — projection compatibility and closure | historical closed record; strict subsystem disposition superseded | `plans/closure/agent-run-worktree-concurrency/006-status.md`; post-closure audit found owner/lineage/idempotency/projection defects, corrected across M007–M009. |
| Agent runs, async delegation, and worktree concurrency | M007 — durable lineage, owner context, fan-out, and authorization corrective pass | closed historical evidence | `plans/closure/agent-run-worktree-concurrency/007-status.md`; implementation `4863765a`. |
| Agent runs, async delegation, and worktree concurrency | M008 — call identity, authoritative projection, and strict corrective closure | closed historical evidence; strict subsystem disposition superseded by M009 | `plans/closure/agent-run-worktree-concurrency/008-status.md`; implementation `5ced31bf`; later exact-head audit found root-turn completion/invocation-scope gaps and push CI `33566227214` failed Workspace Clippy before tests. |
| Agent runs, async delegation, and worktree concurrency | M009 — root completion delivery, invocation scope, and exact-head closure | closed | `plans/closure/agent-run-worktree-concurrency/009-status.md`; accepted final candidate and hosted `CI / verify` evidence recorded there; no registered future plan was unblocked. |
| Agent runtime, model adaptation, and ACP | M017 — corrective integration evidence and closure | closed | `plans/closure/agent-runtime-model-adaptation-acp/017-status.md` |
| Post-audit correctness, simplification, and footprint | C002 — sandbox rights correction and strict closure | closed | `plans/closure/post-audit-correctness-simplification/010-sandbox-rights-correction-status.md`; hosted run `31425564638` |
| Post-audit correctness, simplification, and footprint | C003 — daemon startup, shutdown, and process-lifecycle corrective pass | closed | `plans/closure/post-audit-correctness-simplification/012-status.md`; implementation `0bb7d5b`; no registered future plan was unblocked. |
| Runtime safety, resource control, and footprint | C002 | conditionally closed | `plans/closure/runtime-safety-resource-footprint/010-status.md` |
| Runtime safety — checked edit-history follow-up | M011 — mutation attribution and durable edit checkpoints | closed historical evidence; current correctness disposition owned by M013 | `plans/closure/runtime-safety-resource-footprint/011-status.md`; M013 owns later-discovered same-workspace same-path inter-session attribution and mixed-batch eligibility gaps. |
| Runtime safety — checked edit-history follow-up | M012 — checked Undo/Reapply | closed historical evidence; current strict subsystem disposition owned by M013 | `plans/closure/runtime-safety-resource-footprint/012-status.md`; exact head `4dd1220c` later failed hosted Workspace Clippy before tests. |
| Runtime safety — checked edit-history corrective follow-up | M013 — cross-session checkpoint atomicity and hosted closure corrective pass | closed | `plans/closure/runtime-safety-resource-footprint/013-status.md`; exact candidate `f314c38e`; hosted run `33712437859` / job `100514597927` passed through Workspace tests; no future registered plan was unblocked. |
| Provider connections and Eggpool | M007 — conditional disposition (historical) | superseded by strict closure | `plans/closure/provider-connections/007-status.md`; the record's historical sections preserve the earlier conditional result and hosted Clippy failure `30681164263`; see the strict row below |
| Programmatic tool execution and Tool Programs | M018 — runtime fixture correction (historical) | provisional implementation evidence retained; strict disposition owned by M019 | `plans/closure/tool-programs/018-status.md`; see the M019/M020 rows below |
| Search and eggsearch integration | M001 — current eggsearch request-contract repair | closed | `plans/closure/search-eggsearch-integration/001-status.md`; implementation `acb6ba8`; M002 unblocked |
| Search and eggsearch integration | M002 — external search ownership consolidation | closed | `plans/closure/search-eggsearch-integration/002-status.md`; implementation `e46f97d2`; M003 moved to ready |
| Search and eggsearch integration | M003 — structured contract and compatibility closure | historical closed evidence; current strict disposition superseded by later corrective milestones | `plans/closure/search-eggsearch-integration/003-status.md`; implementation `89dbac7`; M004 corrected the deep-research consumer gap |
| Search and eggsearch integration | M004 — deep-research structured-consumption corrective pass | historical closed implementation evidence; current strict disposition superseded by M005 | `plans/closure/search-eggsearch-integration/004-status.md`; implementation `6f1fa20a`; exact hosted run `31930352527` / job `95124064959` later failed on M004 Clippy and M005 owns remaining SourceCard/workflow fidelity |
| Search and eggsearch integration | M005 — hosted closure and SourceCard fidelity corrective pass | closed | `plans/closure/search-eggsearch-integration/005-status.md`; implementation/final candidate `75ccc70e`; hosted run `32047863303` / job `95439829669` passed through Workspace tests |
| Provider connections and Eggpool | M007 — independent closure ratification and governance reconciliation | closed (strict) | `plans/closure/provider-connections/007-status.md`; accepted revision `c85980e2`; shared hosted run `30931979689` / job `92084050226` passed on attempt 3; earlier conditional record retained as historical evidence |
| Provider connections and Eggpool | M008 — OpenCode Go stable session header corrective pass | closed historical evidence; current direct-call disposition owned by M009 | `plans/closure/provider-connections/008-status.md`; implementation `328c26cb`; typed transport/header behavior remains accepted, but later audit found OpenCode-capable direct callers with empty request context |
| Provider connections and Eggpool | M009 — direct provider session-context closure corrective pass | closed | `plans/closure/provider-connections/009-status.md`; direct research, nested tool, compaction, and remaining provider call sites classified and corrected; no registered future plan was unblocked |
| Programmatic tool execution and Tool Programs | M019 — independent strict closure and evidence ratification | closed | `plans/closure/tool-programs/019-status.md`; accepted revision `c85980e2`; shared hosted run `30931979689` / job `92084050226` |
| Programmatic tool execution and Tool Programs | M020 — canonical child-artifact recovery corrective closure | closed | `plans/closure/tool-programs/020-status.md`; implementation `c85980e2`; covered by the same green hosted run |
| Development verification and release | M007 — minimal verification contract and final closure | closed | `plans/closure/development-verification-release/007-status.md`; accepted revision `c85980e2`; boundary guard fail-open correction; no registered plan was left blocked |
| Memory-to-skill promotion | M001 — habit observation and candidate store | closed historical evidence; current hosted verification disposition owned by M004 | `plans/closure/memory-skill-promotion/001-status.md`; implementation `2f029d8d`; later exact-head audit found the two advisory-lock `OpenOptions` warnings now owned by M004. |
| Memory-to-skill promotion | M002 — user-triggered skill draft and preview | closed | `plans/closure/memory-skill-promotion/002-status.md`; implementation `583c2702`; M003 moved to ready. |
| Memory-to-skill promotion | M003 — approved publication and asset refresh | closed historical evidence; current strict subsystem disposition owned by M004 | `plans/closure/memory-skill-promotion/003-status.md`; implementation `081ae51`; closure recorded the habit Clippy warnings but M004 owns their workstream attribution and exact-head hosted closure. |

Detailed predecessor history is intentionally not duplicated here. Use the source subsystem roadmaps, corrective addenda, and `plans/closure/` records for older milestones.

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

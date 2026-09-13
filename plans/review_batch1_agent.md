# Review: Batch 1 — Agent Context and Execution

**Reviewed**: 2026-09-13
**Files**: agent.md, agent-tool-surface.md, compaction.md, cache-aware-context.md, context-ledger.md, context-compaction-ownership.md, model_profile_task_state.md, goal.md, research.md

## Summary

Nine architecture documents covering the agent orchestration engine, tool surface resolution, compaction pipeline, context packing/ledger, model profiles/task state, goal runtime, and research pipeline. Documents are generally well-maintained with high fidelity to code. The most significant issues are stale field counts in agent.md and a research file-count discrepancy. Most line-number references and struct definitions are accurate within 5 lines.

## Documentation Issues

| # | File | Line | Issue | Action |
|---|------|------|-------|--------|
| D1 | agent.md | 294 | `AgentLoop` stated to have "32 direct fields" — actual count is 30 fields (loop.rs:86–129) | Update field count from 32 → 30 |
| D2 | agent.md | 436 | `SubAgentRequest` struct listing shows 11 fields but code has 13 fields (worker.rs:82–99): missing `run_id: Option<AgentRunId>`, `parent_run_id: Option<AgentRunId>`, and `workspace_locks: Option<Arc<WorkspaceLockTable>>` | Add missing fields to struct listing |
| D3 | agent.md | 324 | `ExecutionLimits.max_tokens` documented as `usize` with default 1,000,000 — code at loop.rs:72 uses `max_tokens` (not `max_total_tokens`), matches doc but field name is `max_tokens` not `max_total_tokens` as sometimes implied | No change needed, name is correct |
| D4 | agent.md | 464 | `ModelRouter` documented with 3 model fields (simple/medium/complex) but struct at router.rs:22–27 has 4 fields: `simple_model`, `medium_model`, `complex_model` (documented) plus the enabled flag. The doc correctly notes `complex_model` as the `model` config field | Verify `complex_model` field description |
| D5 | research.md | 19 | States "14 files + sources/ subdirectory with 8 adapters" — actual files in `src/research/` are 13 (coordinator, types, store, extract, claims, verify, synthesis, llm, templates, service, runtime, triggers, error); sources/ has 7 adapter files (advisory, crates_io, docs_rs, eggsearch, github, local_repo, url) plus mod.rs. `AdvisorySource` implements `ResearchSourceAdapter` but is not registered in the coordinator | Update file count to 13; clarify sources/ has 7 adapters (6 registered + 1 unregistered advisory) |
| D6 | research.md | 176 | `ResearchMode` listed as 8 variants in the doc table — code at types.rs:25–34 has exactly 8 variants. However the doc does not document `SourceType` line reference correctly: doc says `:111` but actual `SourceType` enum starts at types.rs:111 | Line ref :111 is correct for SourceType |

## Code Issues Found

| # | Module | Bug/Issue | Location | Severity |
|---|--------|-----------|----------|----------|
| C1 | agent | `AgentLoop` field count mismatch: doc claims 32 but code has 30. Not a code bug but indicates the doc was not updated after field removal/refactoring | agent.md:294, loop.rs:86–129 | LOW |
| C2 | agent | `SubAgentRequest` struct has 3 undocumented fields (`run_id`, `parent_run_id`, `workspace_locks`). These are scheduler/workspace integration fields added after the doc was written | agent.md:436, worker.rs:82–99 | LOW |
| C3 | research | `AdvisorySource` implements `ResearchSourceAdapter` (advisory.rs:93) but is not registered in the coordinator. This is documented correctly in research.md (line 87–89) but could confuse readers who count impls | research.md:82–85, coordinator.rs:37–52 | LOW |
| C4 | compaction | `CompactionMode` doc at compaction.md:129 lists `Programmatic`, `Agent`, `Hybrid` — verified in code at compaction.rs:654–659. No issues | compaction.rs:654 | NONE |
| C5 | model_profile | `ResolvedModelProfile` doc at model_profile_task_state.md:132 lists ~18 fields but code at types.rs:9–36 has 19 fields (including `orchestration_tier` which is not in the doc table) | model_profile_task_state.md:132–157, types.rs:9–36 | LOW |

## Improvement Opportunities

| # | Module | Opportunity | Impact |
|---|--------|-------------|--------|
| I1 | agent | agent.md should add a field-count cross-check comment (e.g., "N fields as of YYYY-MM-DD") so stale counts are caught during reviews | Prevents drift |
| I2 | agent | agent.md's `SubAgentRequest` listing should include the scheduler-owned `run_id` and `parent_run_id` fields to document the durable delegation path fully | Completeness |
| I3 | research | research.md should explicitly state that `AdvisorySource` is an unregistered adapter available for direct use but not part of the standard coordinator pipeline | Clarity |
| I4 | model_profile | `ResolvedModelProfile` field table should include `orchestration_tier` and its purpose (`OrchestrationTier` type for model orchestration classification) | Completeness |
| I5 | compaction | Compaction.md's `ContextTracker` struct listing should note the `model` field (used for model-aware token estimation) which is set via `with_model()` | Accuracy |

## Stale Content to Prune

| # | File | Content | Reason |
|---|------|---------|--------|
| S1 | agent.md:294 | "32 direct fields" count | Stale — actual is 30; fields were removed during refactoring |
| S2 | agent.md:436 | SubAgentRequest field listing (11 fields) | Stale — code has 13 fields with scheduler/workspace additions |

## Verified Claims (spot-checked)

| Claim | Source | Status |
|-------|--------|--------|
| AgentLoop at loop.rs:86 | loop.rs:86 | ✅ |
| Agent struct at definition.rs:78 | definition.rs:78 | ✅ |
| AgentRuntimeKind 6 variants | definition.rs:30–44 | ✅ |
| Capability enum 12 variants | tool_surface.rs:15–28 | ✅ |
| ToolOmissionReason 6 variants | tool_surface.rs:105–112 | ✅ |
| ResolvedToolSurface at tool_surface.rs:131 | tool_surface.rs:131 | ✅ |
| CompactionPolicy 5 policies with documented budgets | compaction.rs:662–697 | ✅ |
| CompactionMode 3 variants | compaction.rs:654–659 | ✅ |
| EvidenceKind 10 variants | compaction.rs:1161–1172 | ✅ |
| ContextCompactionResult 7 status variants | compaction.rs:833–839 | ✅ |
| MAX_CONTINUATIONS = 32 | turn_completion.rs:181 | ✅ |
| GoalStatus 7 variants | goal/model.rs:8–16 | ✅ |
| Goal struct fields | goal/model.rs:52–79 | ✅ |
| GoalStore methods and line numbers | goal/store.rs:160–560 | ✅ |
| ResearchMode 8 variants | research/types.rs:25–34 | ✅ |
| ClaimType 7 variants | research/types.rs:189–197 | ✅ |
| ResearchOutputProfile 5 variants | research/types.rs:56–62 | ✅ |
| Runtime constants (MAX_CHILD_TASKS=3, MAX_SOURCES=32, etc.) | research/runtime.rs:11–15 | ✅ |
| TodoState 4 fields | task_state/mod.rs:33–38 | ✅ |
| TodoStatus 5 variants | task_state/mod.rs:16–22 | ✅ |
| TaskStatePolicy preset policies and line refs | model_profile/types.rs:62–124 | ✅ |
| Context cache-aware packer module structure | context/mod.rs:1–15 | ✅ |
| Context-compaction-ownership single-owner claim | context/compaction.rs (canonical), agent/compaction.rs (re-export only) | ✅ |

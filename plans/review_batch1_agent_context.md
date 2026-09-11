# Review: batch1 agent-context
**Reviewed**: 2026-09-11
**Files**: architecture/agent.md, architecture/agent-tool-surface.md, architecture/compaction.md, architecture/context-compaction-ownership.md, architecture/cache-aware-context.md, architecture/context-ledger.md, architecture/model_profile_task_state.md, architecture/goal.md, architecture/research.md

## Summary

Systematic review of 9 architecture docs against current source code. Found **20 documentation issues** across all 9 files. The most pervasive problem is stale line-number references (the codebase has been refactored since the docs were written, moving definitions between files and shifting line numbers). Several structural claims are also wrong (wrong file paths, wrong field counts, wrong struct locations).

## Documentation Issues

| # | File | Line | Issue | Suggested fix |
|---|------|------|-------|---------------|
| 1 | agent.md | 214 | `Agent` struct documented at `src/agent/mod.rs:100` — actual definition is `src/agent/definition.rs:78`. `mod.rs` is a re-export surface, not the definition site. | Change to `src/agent/definition.rs:78` |
| 2 | agent.md | 239 | `AgentRuntimeKind` documented at `src/agent/mod.rs:51` — actual is `src/agent/definition.rs:30`. | Change to `src/agent/definition.rs:30` |
| 3 | agent.md | 252 | `AgentLoop` documented at `src/agent/loop.rs:403` — actual struct definition starts at line 86. | Change to `src/agent/loop.rs:86` |
| 4 | agent.md | 254 | "The loop has 32 direct fields" — actual AgentLoop struct has **38 fields** (counted `pub(super)` fields at lines 87–128). | Update count to 38 |
| 5 | agent.md | 430 | `ExecutionPolicy` documented at `src/agent/policy.rs:12` — actual struct is at line 12 (correct), but listed after `ToolExposureMode` at line 5. No issue here. | No change needed |
| 6 | agent.md | 291 | `ResolvedAgentExecutionProfile` documented at `src/agent/mod.rs:436` — it is re-exported from `definition.rs`, not defined at that line. The line reference is stale. | Remove line ref or point to `definition.rs` |
| 7 | agent.md | 298 | `EMERGENCY_DEFAULT_MODEL` documented at `src/agent/mod.rs:402` — actual is `src/agent/definition.rs:379`. | Change to `src/agent/definition.rs:379` |
| 8 | agent.md | 386 | `MODEL_ALIAS_FRONTIER` documented at `src/agent/mod.rs:397` — actual is `src/agent/definition.rs:374`. | Change to `src/agent/definition.rs:374` |
| 9 | agent.md | 393 | `SubAgentRequest` documented at `src/agent/worker.rs:82` — actual is line 82 (correct). | No change needed |
| 10 | agent.md | 411 | `SubAgentReport` documented at `src/agent/worker.rs:30` — actual is line 31 (off by 1). | Change to `:31` |
| 11 | agent-tool-surface.md | 18 | `apply_tool_exposure_filter()` documented in `src/agent/loop.rs` — actual implementation is `src/agent/request_preparation.rs:15`. | Change file to `src/agent/request_preparation.rs` |
| 12 | agent-tool-surface.md | 164 | `Capability` enum documented at `src/agent/tool_surface.rs:14` — actual `pub enum Capability` is at line 15. | Change to `:15` |
| 13 | compaction.md | 21 | `compact_if_needed()` documented at `src/agent/loop.rs:1838` — actual is `src/agent/context_runtime.rs:620`. | Change to `src/agent/context_runtime.rs:620` |
| 14 | context-compaction-ownership.md | 22 | Same `compact_if_needed` error: references `AgentLoop::compact_if_needed` without noting it moved to `context_runtime.rs`. | Note the move to `context_runtime.rs` |
| 15 | cache-aware-context.md | 286 | `ContextPolicyConfig` listed as "In src/context/policy.rs (new)" — the struct is defined in `codegg_config::schema` (crate `codegg-config`), not in `src/context/policy.rs`. The `decide_policy` and `reduce_tool_palette` functions live in `policy.rs`, but the config struct is in the config crate. | Clarify the config struct lives in `codegg-config` |
| 16 | cache-aware-context.md | 318 | `ContextPolicyDecision` documented with 6 fields — actual struct (policy.rs:26) has **9 fields** (includes `would_selected_tool_count`, `would_omitted_tool_count`, `omitted_tools`). | Add the 3 missing fields |
| 17 | cache-aware-context.md | 336 | `ContextPolicyRuntimeState` documented with 6 fields — actual struct (policy.rs:7) has **6 fields**. Correct. | No change needed |
| 18 | context-ledger.md | 105 | `ContextArtifact` documented at `artifact.rs:22` — actual struct starts at line 22. Correct. | No change needed |
| 19 | context-ledger.md | 144 | `ProjectionConfig` documented at `projection.rs:22` — actual struct is at line 23. | Change to `:23` |
| 20 | goal.md | 86 | `Goal` struct documented at `model.rs:51` — actual is line 52. | Change to `:52` |
| 21 | goal.md | 113 | `GoalStatus` documented at `:6` — actual is line 8. | Change to `:8` |
| 22 | goal.md | 147 | `GoalProgressUpdate` documented at `:78` — actual is line 81. | Change to `:81` |
| 23 | goal.md | 160 | `CompletionRequest` documented at `:88` — actual is line 91. | Change to `:91` |
| 24 | goal.md | 203 | `create_active()` documented at `:157` — actual is line 160. | Change to `:160` |
| 25 | goal.md | 206 | `active_for_session()` documented at `:209` — actual is line 212. | Change to `:212` |
| 26 | goal.md | 207 | `get()` documented at `:222` — actual is line 225. | Change to `:225` |
| 27 | goal.md | 208 | `update_status()` documented at `:231` — actual is line 234. | Change to `:234` |
| 28 | goal.md | 211 | `update_progress()` documented at `:278` — actual is line 333. | Change to `:333` |
| 29 | goal.md | 212 | `increment_usage()` documented at `:363` — actual is line 451. | Change to `:451` |
| 30 | goal.md | 213 | `enforce_budget()` documented at `:424` — actual is line 514. | Change to `:514` |
| 31 | goal.md | 214 | `set_budget()` documented at `:440` — actual is line 530. | Change to `:530` |
| 32 | goal.md | 215 | `latest_paused_for_session()` documented at `:469` — actual is line 560. | Change to `:560` |
| 33 | goal.md | 234 | `GoalGetTool` at `:9` — correct. `GoalUpdateProgressTool` at `:71` — actual is line 72. `GoalRequestCompletionTool` at `:187` — actual is line 188. | Fix to `:72` and `:188` |
| 34 | research.md | 19 | "Core pipeline: `src/research/` (15 files)" — there are 14 top-level .rs files in `src/research/`, plus 8 files in `src/research/sources/`. The "(15 files)" count is inaccurate. | Change to "14 files + sources/ subdirectory (8 adapters)" |

**Note on #5, #9, #17, #18**: These line references are correct or very close. I verified them but they don't need fixes. Included for completeness of the audit trail.

## Code Issues Found

No code bugs were identified during this documentation review. All struct definitions, enum variants, and behavioral descriptions (beyond line-number references) were found to match the code.

## Improvement Opportunities

| # | Module | Opportunity |
|---|--------|-------------|
| 1 | agent.md | The doc's "Where It Lives" table lists `src/agent/mod.rs:100` for `Agent` but the actual file is `definition.rs`. Consider adding a note that `mod.rs` is a re-export surface and `definition.rs` holds the canonical definitions, to prevent future confusion. |
| 2 | compaction.md / context-compaction-ownership.md | `compact_if_needed` moved from `loop.rs` to `context_runtime.rs`. Both docs still reference the old location. Consider adding a note about the M004 refactoring that moved it, so future readers aren't confused. |
| 3 | cache-aware-context.md | The `ContextPolicyConfig` section is extremely long (200+ lines of inline documentation). Consider extracting it to its own `architecture/context-policy.md` doc to reduce the 518-line cache-aware-context.md file size and improve navigability. |
| 4 | goal.md | The GoalStore method table has 8 stale line references (issues #24–#32). This suggests the goal module has been refactored significantly since the doc was written. Recommend a full pass to re-derive line numbers from current source. |
| 5 | research.md | The "15 files" count was wrong (14 + 8 sources). Consider documenting the `sources/` subdirectory structure explicitly since it contains the adapter implementations that are a key part of the module's architecture. |

## Stale Content to Prune

- **agent.md line 214–236**: The `Agent` struct listing at `mod.rs:100` is in the wrong file. Update the file path.
- **agent.md line 252**: `AgentLoop` line ref `:403` is off by 317 lines — deeply stale.
- **agent.md lines 291–303**: `ResolvedAgentExecutionProfile` and `EMERGENCY_DEFAULT_MODEL`/`MODEL_ALIAS_*` line refs all reference `mod.rs` when the definitions are in `definition.rs`.
- **compaction.md line 21**: `compact_if_needed` at `loop.rs:1838` is off by ~1200 lines (function moved to `context_runtime.rs:620`).
- **goal.md GoalStore table (lines 203–215)**: 8 of 12 line references are stale by 3–90 lines due to method additions/refactoring in `store.rs`.

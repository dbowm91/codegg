# Development Status Tracker

**Status**: UPDATED 2026-05-29
**Purpose**: Track truly completed items vs. active development items vs. future work.

Items marked **DONE** were implemented and verified.
Items marked **ACTIVE** are real development tasks for the current sprint/roadmap.
Items marked **FUTURE** are explicitly deferred to later phases.

---

## DONE - Verified Completed Items

| Item | Location | Verification |
|------|----------|--------------|
| Subagent role-specific output contracts | `src/agent/prompt.rs:398-408` | `subagent_output_contract()` function with explore/review/debug/test/security/planner/executor roles; wired into `assemble_system_prompt_with_profile()` at line 317 and `base_prompt_parts()` at line 341 |
| Architecture docs for subagent contracts | `architecture/agent.md` | Subagent Output Contracts section added |
| Sensitive paths enforcement | `src/security/mod.rs:25` + `src/agent/loop.rs:506,~610` | `matches_sensitive_path()` function; wired into `check_tool_permission()` - sensitive paths escalate Allow→Ask |
| Agent role field | `src/config/schema.rs:283` + `src/agent/mod.rs:32` | `AgentConfig.role: Option<String>` and `Agent.role: Option<String>`; wired through `merge_agent_config()` and `agent_from_config()` |
| Profile-aware tool filtering (disabled_tools) | `src/agent/policy.rs:26` + `src/agent/loop.rs:767-775` | `disabled_tools: Option<Vec<String>>` in `ExecutionPolicy`; `apply_tool_exposure_filter()` removes disabled tools after exposure mode filter |
| `defer_loading` in ToolDefinition | `src/provider/mod.rs:204` | `defer_loading: Option<bool>` with `#[serde(default)]` |
| `defer_loading` in Tool trait | `src/tool/mod.rs:70-72` + `src/tool/catalog.rs:27` | `fn defer_loading(&self) -> bool { false }` in Tool trait; `ToolMetadata::from_tool()` reads it |
| Stale test detection | `src/session/state.rs:60` | `TestState::Stale` exists; `FileChanged` events mark tests as stale |
| TodoUpdated event | `src/bus/events.rs:49` | `AppEvent::TodoUpdated { session_id: String }` exists |
| Todo persistence wiring | `src/session/store.rs:1550-1744` | Full `TodoStore` with list/set/add/update/remove/clear; `to_session_input()` and `from_session_model()` exist; load on resume at `src/core/mod.rs:202`; save on write at `src/tool/todo.rs:167-175` |
| Todo task_state_policy via model_profile | By design | Works through `model_profile` override - no dedicated top-level config needed |

---

## ACTIVE - Items Requiring Development

These are real development tasks, not truly "deferred". They are active work for the current sprint.

### polish.md

| Item | Notes |
|------|-------|
| Prompt assembly cleanup verification | Verify no duplicate control paragraphs after compaction or resume. Add dedupe mechanism if needed. |

### tui.md

| Item | Notes |
|------|-------|
| Dedicated plan panel side rendering | No dedicated side panel for goal/plan exists. Context inspector exists at `src/tui/app/mod.rs:3100`. |
| Changed files panel | `changed_files: Vec<ChangedFileSummary>` exists in TuiSessionState but needs TUI rendering |
| File-level diff navigation | Partial - `/review` and `/diff` commands exist but could be enhanced |
| Hunk-level accept/reject | Optional per plan - low priority |

### security.md

| Item | Notes |
|------|-------|
| Auto-invocation of security-review agent | Agent exists at `src/agent/mod.rs:285-317` but not automatically invoked. Need to wire trigger heuristics. |

### deepresearch.md

| Item | Notes |
|------|-------|
| `/research` TUI slash command | Need to add to TUI command registry |
| TUI research browser | Not implemented |
| `/research-runs`, `/research-open`, `/research-show` commands | Not implemented |
| ResearchService exposed to agent system | Not implemented |
| Planner/reviewer `ResearchTool` integration | Not implemented |
| Trigger heuristics for research invocation | Not implemented |
| Research runs list view | Not implemented |
| Run details, sources, claims, report views | Not implemented |

### tooluse.md

| Item | Notes |
|------|-------|
| Provider capability detection | Need `ProviderCapabilities::for_provider()` to detect defer_loading support |
| Immediate vs deferred tool partitioning in AgentLoop | Need to separate tools into immediate/deferred arrays in ChatRequest |
| Anthropic `defer_loading` support | Need provider-level support |
| OpenAI fallback for non-supporting models | Need fallback for providers without defer_loading |
| Provider-level deferred_tools array | Need to wire deferred_tools into actual provider requests |
| MCP tool catalog deferral | MCP tools don't yet set defer_loading based on catalog |
| `tools.defer_loading` config option | Need config schema for tools.defer_loading |
| `tools.always_loaded` config option | Need config schema for tools.always_loaded |
| `tools.search_mode` config option | Need config schema for tools.search_mode |
| `tools.max_initial_tools` config option | Need config schema for tools.max_initial_tools |

### prompting.md

| Item | Notes |
|------|-------|
| Profile-aware tool exposure mode filtering | `apply_tool_exposure_filter()` exists but full Curated/MinimalWithDiscovery/Full modes not fully wired |

---

## FUTURE - Explicitly Deferred to Later Phases

These are planned for future work and are explicitly NOT in the current sprint.

| Item | Plan | Notes |
|------|------|-------|
| CratesIoSource adapter | deepresearch.md | Future adapter |
| GitHubSource adapter | deepresearch.md | Future adapter |
| DocsRsSource adapter | deepresearch.md | Future adapter |
| AdvisorySource adapter | deepresearch.md | Future adapter |
| SearchProviderSource (Tavily/Brave/SerpAPI/Kagi) | deepresearch.md | Future adapter |
| eggsact crate integration | tooluse.md | Future/separate project |
| MathEvalTool, TextInspectTool, ValidateJsonTool | tooluse.md | Future - depends on eggsact |
| BM25 ranking for tool search | tooluse.md | v2 upgrade path |
| Semantic embeddings search | tooluse.md | v3, requires model |
| TUI sidebar checklist rendering | todos.md | Nice to have - event exists |

---

## Summary

| Category | Count | Status |
|----------|-------|--------|
| Truly completed (verified in code) | 11 | DONE |
| Real development tasks | 17 | ACTIVE |
| Explicitly future/deferred | 10 | FUTURE |

**Total**: 38 discrete items across 7 plan files
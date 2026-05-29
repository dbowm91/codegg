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
| Changed files panel | `src/tui/components/dialogs/review.rs` | `ReviewDialog` component with `FileList` and `DiffView` modes, `/review` command, status bar integration at `src/tui/app/mod.rs:1474-1478` |
| File-level diff navigation | `src/tui/components/dialogs/review.rs` + `src/tui/app/mod.rs:4865-4924` | `/review` opens file list, Enter opens per-file diff via `similar` crate. `/diff <path>` runs `git diff` |
| Profile-aware tool exposure mode filtering | `src/agent/policy.rs:5` + `src/agent/loop.rs:736-778` | `ToolExposureMode` enum (Full/Curated/MinimalWithDiscovery), per-profile mapping in `default_tool_exposure()`, `apply_tool_exposure_filter()` with allowlists, 5 tests |
| Prompt assembly dedupe | `src/model_profile/policy.rs:42-72` | `content_already_present()` helper scans messages for duplicate text. Guards in `inject_control_text()` and `push_control_instruction()`. 5 dedup tests |
| Security-review auto-invocation | `src/agent/loop.rs` + `src/config/schema.rs:832` | `maybe_spawn_security_review()` triggers on high-signal findings or sensitive path edits. Config-gated via `auto_invoke_review_agent: bool`. Non-blocking `tokio::spawn`. Session-end review trigger |
| Provider capability detection | `src/provider/mod.rs:111-140` | `ProviderCapabilities` struct with `supports_defer_loading`, `supports_tool_references`, `max_tools_per_request`. `for_provider()` method with Anthropic/OpenAI defaults |
| Tool deferral partitioning | `src/agent/loop.rs` | `build_tool_definitions()` partitions into immediate vs deferred based on provider capabilities, config, and `defer_loading` flag. Deferred tools stored in `deferred_tool_definitions` field |
| Tool deferral config | `src/config/schema.rs` | `ToolDeferralConfig` struct: `defer_loading`, `always_loaded`, `search_mode`, `max_initial_tools`. Wired into `Config.tool_deferral` |
| `/research` TUI command | `src/tui/command.rs` + `src/tui/app/mod.rs` | `/research` command registered, `handle_research_command()` parses flags and spawns async research |
| `/research-runs` and `/research-open` commands | `src/tui/command.rs` + `src/tui/app/mod.rs` | Commands registered, handlers list runs and display run details |
| ResearchService wrapper | `src/research/service.rs` | `ResearchService` struct with `answer_for_agent()`, `create_report()`, `list_runs()`, `load_run()`. 8 unit tests |

---

## ACTIVE - Items Requiring Development

These are real development tasks, not truly "deferred". They are active work for the current sprint.

### tui.md

| Item | Notes |
|------|-------|
| Dedicated plan panel side rendering | No dedicated side panel for goal/plan exists. Context inspector exists at `src/tui/app/mod.rs:3100`. |
| Hunk-level accept/reject | Optional per plan - low priority |

### deepresearch.md

| Item | Notes |
|------|-------|
| TUI research browser | Not implemented - needs dedicated browser view for runs, sources, claims, reports |
| Planner/reviewer `ResearchTool` integration | Not implemented - agent system needs tool to call ResearchService |
| Trigger heuristics for research invocation | Not implemented - auto-invoke research when task touches unknown APIs/libs |
| Run details, sources, claims, report views | Not implemented - detailed views for research artifacts |
| `/research-show` command | Not implemented - `/research-show report <run_id>`, `/research-show handoff <run_id>`, etc. |

### tooluse.md

| Item | Notes |
|------|-------|
| Anthropic `defer_loading` support | Need provider-level support to send `defer_loading` in API request |
| OpenAI fallback for non-supporting models | Need fallback for providers without defer_loading |
| Provider-level deferred_tools array | Need to wire deferred_tools into actual provider request structs |
| MCP tool catalog deferral | MCP tools don't yet set defer_loading based on catalog |

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
| Truly completed (verified in code) | 24 | DONE |
| Real development tasks | 10 | ACTIVE |
| Explicitly future/deferred | 10 | FUTURE |

**Total**: 44 discrete items across 7 plan files

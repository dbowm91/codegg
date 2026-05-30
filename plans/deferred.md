# Development Status Tracker

**Status**: UPDATED 2026-05-30
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
| Anthropic `defer_loading` support | `src/provider/mod.rs:250-282` | `to_anthropic()` serializes `defer_loading` field in tool definitions when present |
| OpenAI `defer_loading` support | `src/provider/mod.rs:251-268` | `to_openai()` serializes `defer_loading` field in function definitions when present |
| OpenAI-compatible fallback | `src/provider/openai_compatible.rs:129-131` | `build_body()` uses `t.to_openai()` for consistent serialization; non-supporting providers get all tools in single array |
| Provider-level deferred_tools in requests | `src/agent/loop.rs:1512,1625` | `deferred_tool_definitions` included in returned definitions for both cache-hit and non-cache paths |
| MCP tool catalog deferral | `src/agent/loop.rs:1442-1455,1533-1538` | MCP tools get `defer_loading: Some(true)` from `ToolCatalog::is_deferred()`; separated into immediate vs deferred in `build_tool_definitions()` |
| TUI research browser | `src/tui/components/dialogs/research.rs` | `ResearchBrowserDialog` with RunsList, RunDetail, and ReportView modes; 619 lines; full keyboard navigation |
| `/research-show` command | `src/tui/command.rs` + `src/tui/app/mod.rs` | `/research-show` registered, opens ResearchBrowserDialog with run details |
| Research trigger heuristics | `src/research/triggers.rs` | `analyze_trigger()` with 5 heuristic rules (comparison, unknown API, security, architecture, previous failures); 10 unit tests |
| Research auto-invoke integration | `src/research/service.rs` | `should_auto_invoke()` method runs trigger analysis and returns `Option<ResearchRequest>` |
| Research trigger config | `src/config/schema.rs` | `ResearchConfig` with `triggers: Option<ResearchTriggerConfig>` (enabled, min_confidence) |
| Task panel enhanced rendering | `src/tui/app/mod.rs:1480-1517` | Test state and active tool sections added to task panel |
| TUI sidebar checklist | `src/tui/components/sidebar.rs` | ASCII checkbox indicators ([x], [>], [ ], [!], [-], [?]) with theme-aware colors |
| CratesIoSource adapter | `src/research/sources/crates_io.rs` | Fetches crate metadata from crates.io API; 3 unit tests |
| GitHubSource adapter | `src/research/sources/github.rs` | Supports repo/file/issue URL patterns via GitHub REST API; rate limit handling; 8 unit tests |
| DocsRsSource adapter | `src/research/sources/docs_rs.rs` | Fetches docs.rs pages with html2text conversion; 4 unit tests |
| AdvisorySource adapter | `src/research/sources/advisory.rs` | Fetches RustSec advisory metadata via crates.io versions API; yanked version detection |
| SearchProviderSource adapter | `src/research/sources/search_provider.rs` | Tavily, Brave, SerpAPI, Kagi support with provider-specific auth |
| BM25 ranking for tool search | `src/tool/catalog.rs` | `SearchMode::BM25` with tokenization, IDF computation, BM25 scoring (k1=1.5, b=0.75); 19 unit tests |
| BM25 config wiring | `src/config/schema.rs` + `src/agent/loop.rs` | `search_mode: "bm25"` in `ToolDeferralConfig` activates BM25 ranking |

---

## ACTIVE - Items Requiring Development

These are real development tasks, not truly "deferred". They are active work for the current sprint.

### tui.md

| Item | Notes |
|------|-------|
| Hunk-level accept/reject | Optional per plan - low priority |

### deepresearch.md

| Item | Notes |
|------|-------|
| Model-backed evidence extraction | Templates exist in `src/research/templates.rs` but are unused; pipeline uses deterministic fallbacks |
| Model-backed claim construction | Currently trivial (1 claim per evidence span); needs LLM integration |
| Semantic citation verifier | Structural verifier exists; semantic support-by-citation checking not implemented |
| Research refresh/rerun | Rerun source fetches and diff changed claims - not implemented |
| Re-synthesis from existing evidence | Generate different output profiles from same evidence - partially supported via templates |
| SQLite metadata index for research runs | Optional for session search and TUI lists |

### tooluse.md

| Item | Notes |
|------|-------|
| eggsact crate integration | External project - separate crate, not in this repo |
| MathEvalTool, TextInspectTool, ValidateJsonTool | Depends on eggsact crate completion |
| Semantic embeddings search | v3 upgrade path, requires model integration |

---

## FUTURE - Explicitly Deferred to Later Phases

These are planned for future work and are explicitly NOT in the current sprint.

| Item | Plan | Notes |
|------|------|-------|
| eggsact crate integration | tooluse.md | Future/separate project |
| MathEvalTool, TextInspectTool, ValidateJsonTool | tooluse.md | Future - depends on eggsact |
| Semantic embeddings search | tooluse.md | v3, requires model |
| Optional server API endpoints for research runs | deepresearch.md | Future follow-up |

---

## Summary

| Category | Count | Status |
|----------|-------|--------|
| Truly completed (verified in code) | 44 | DONE |
| Real development tasks | 9 | ACTIVE |
| Explicitly future/deferred | 4 | FUTURE |

**Total**: 57 discrete items across 7 plan files

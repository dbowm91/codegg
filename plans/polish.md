# Codegg Harness Architecture Polish Plan

**Status**: ~95% COMPLETE (verified 2026-06-02)

| Phase | Status | Notes |
|-------|--------|-------|
| 1. ExecutionPolicy from profile | **DONE** | `src/agent/policy.rs` with 6 tests |
| 2. Context tracking improvements | **PARTIAL** | ContextFrame done, tool pruning done, token estimator model wiring not wired |
| 3. Adaptive tool exposure | **DONE** | Full/Curated/MinimalWithDiscovery with 5 tests |
| 4. TaskEnvelope router | **DONE** | `src/agent/router.rs` with classifier tests |
| 5. Subagent delegation contracts | **DONE** | `SubAgentReport` + `to_compact_text()` + compact parent context |
| 6. Prompt assembly cleanup | **DONE** | `content_already_present()` dedup with 5 tests |
| 7. Config and documentation | **PARTIAL** | Example config stale, architecture docs not updated |

Remaining items:
- ~~Wire `ContextTracker::with_model()` from active model~~ DONE (2026-06-02)
- ~~Fix example config field names~~ DONE (2026-06-02)
- ~~Update `architecture/agent.md`~~ DONE (2026-06-02)
- ~~Subagent budget enforcement~~ DONE (2026-06-02)

Audience: smaller implementation model such as Mimo 2.5.

Goal: refine Codegg's coding harness so model-specific behavior, context management, subagent delegation, and tool exposure are governed by a coherent policy layer instead of scattered heuristics. Preserve existing behavior where possible, add tests around every policy change, and keep the work incremental.

Repository context: `dbowm91/codegg`, Rust codebase. Relevant modules include `src/agent/loop.rs`, `src/agent/router.rs`, `src/agent/compaction.rs`, `src/agent/prompt.rs`, `src/agent/worker.rs`, `src/model_profile/*`, `src/tool/mod.rs`, `src/tool/catalog.rs`, `src/task_state/*`, and config schemas under `src/config/schema.rs`.

## Current architecture summary

Codegg already has the right primitives:

- `ResolvedModelProfile` in `src/model_profile/types.rs` captures model family, prompt profile, context window, tool reliability, instruction adherence, patch reliability, late system message support, small patch preference, explicit tool contract behavior, post-tool continuation nudge behavior, max parallel tools, preferred/disabled tools, and task-state policy.
- `ModelProfileResolver` in `src/model_profile/resolve.rs` infers broad built-in profiles and applies config overrides.
- `model_profile::policy` injects startup tool contracts, patch discipline, and todo discipline.
- `AgentLoop` in `src/agent/loop.rs` handles routing, prompt setup, tool definitions, compaction, streaming, tool execution, permission checks, security hints, todo reminders, snapshots, MCP calls, and event persistence.
- `ContextTracker` and compaction strategies live in `src/agent/compaction.rs`.
- `ModelRouter` in `src/agent/router.rs` provides a first-pass simple/medium/complex classifier.
- `ToolRegistry` exposes many tools and has `tool_search`/catalog support for discoverability.
- `SubAgentPool` and `SubAgentSpawner` in `src/agent/worker.rs` provide background subagent execution with depth and concurrency limits.

The problem is composition. Policy decisions are split across model profiles, router heuristics, prompt injection, tool filtering, compaction, todo reminders, and loop heuristics. The desired end state is a single per-turn execution policy derived from the active model, agent, task envelope, config, and plan mode.

## Non-goals

Do not rewrite the entire agent loop in one pass.

Do not remove existing tool names or break existing config files unless compatibility shims are added.

Do not change provider APIs unless required for a narrowly scoped improvement.

Do not implement a full multi-agent team framework in this pass. Keep team features experimental; focus on single subagent delegation contracts and context isolation.

Do not introduce large external dependencies unless strongly justified.

## Phase 1: make model profile policy authoritative

### Objective

Move model-specific execution behavior out of scattered conditionals and into `ResolvedModelProfile` plus a new per-turn `ExecutionPolicy` or `RunPolicy` object.

### Files to inspect first

- `src/model_profile/types.rs`
- `src/model_profile/resolve.rs`
- `src/model_profile/policy.rs`
- `src/agent/loop.rs`
- `src/agent/router.rs`
- `src/config/schema.rs`
- `codegg.example.jsonc`

### Add or extend types

Add a new module, preferably `src/agent/policy.rs`, or place under `src/model_profile/execution.rs` if that fits current organization better.

Define something close to:

```rust
#[derive(Debug, Clone)]
pub struct ExecutionPolicy {
    pub model: String,
    pub prompt_profile: PromptProfileKind,
    pub context_window: usize,
    pub compaction_threshold: f64,
    pub reserved_output_tokens: usize,
    pub max_tool_result_tokens: usize,
    pub max_parallel_tools: usize,
    pub expose_tool_search: bool,
    pub initial_tool_mode: ToolExposureMode,
    pub allow_bootstrap_tool: bool,
    pub allow_post_tool_continue_nudge: bool,
    pub prefer_user_control_messages: bool,
    pub supports_late_system_messages: bool,
    pub task_state_policy: TaskStatePolicy,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolExposureMode {
    Full,
    Curated,
    MinimalWithDiscovery,
}
```

The names can be adjusted to fit style, but the concept should remain: one resolved object controls model-specific context/tool behavior for the current run.

### Extend `ResolvedModelProfile`

In `src/model_profile/types.rs`, add optional config-driven fields if they do not already exist:

```rust
pub default_context_window: Option<usize>,       // or use existing context_window
pub reserved_output_tokens: Option<usize>,
pub compaction_threshold: Option<f64>,
pub max_tool_result_tokens: Option<usize>,
pub tool_exposure_mode: Option<ToolExposureModeConfig>,
pub allow_bootstrap_tool: Option<bool>,
pub allow_post_tool_continue_nudge: Option<bool>,
```

If adding new public config fields is too broad, keep these internal to `ExecutionPolicy::from_profile` at first and derive defaults from existing profile fields.

Suggested defaults:

- Frontier reasoning models: context window from profile/config or `128_000`, threshold `0.85`, reserved output `12_000`, max tool result tokens `8_000`, tool mode `Curated`, parallel tools from config or profile.
- Long-context planner models: context window from profile/config or `512_000` if unknown, threshold `0.70`, reserved output `16_000`, tool mode `Curated`, prefer larger read/search budgets but still summarize outputs.
- Fast/tool-fragile models: context window `64_000` or `128_000`, threshold `0.70`, reserved output `8_000`, max tool result tokens `4_000`, tool mode `MinimalWithDiscovery`, max parallel tools `1` or `2`, bootstrap allowed, post-tool nudge allowed.
- Local strict models: conservative context window `32_000` or config override, threshold `0.65`, reserved output `4_000`, max tool result tokens `2_000`, tool mode `MinimalWithDiscovery`, max parallel tools `1`, explicit tool contract enabled.
- Summarizer profile: minimal tools or no tools, low max output, disabled todos unless explicitly configured.

### Update resolver

In `src/model_profile/resolve.rs`, enrich built-in profiles enough that `ExecutionPolicy` can make useful decisions. Existing built-ins currently leave many fields as `None`; preserve config override behavior.

Important: do not hardcode exact vendor limits as truth unless already known in config. Treat these as safe operational defaults. Users can override via config.

### Integrate in `AgentLoop`

In `src/agent/loop.rs`:

1. Resolve `model_profile` after model routing and agent config have selected the final model.
2. Build `ExecutionPolicy` from model profile and config.
3. Use that policy for:
   - `ContextTracker` context limit and threshold.
   - reserved output token budget in overflow detection.
   - max tool result pruning.
   - max parallel tool calls.
   - whether synthetic bootstrap `list .` is allowed.
   - whether post-tool continuation nudges are allowed.
   - whether late system messages should be avoided.

Current code has several hard-coded behaviors to replace gradually:

- `ContextTracker::new(128_000, 0.85)` in `AgentLoop::new`.
- `detect_overflow(... reserved)` default of `10_000`.
- `prune_tool_outputs(messages, 10_000)`.
- `MAX_TOOL_RESULT_BYTES: 512 * 1024` in `execute_tool_calls`.
- synthetic bootstrap `list .` behavior for repo-like prompts.
- generic post-tool continuation nudges.

Do not remove these behaviors outright. Gate them through `ExecutionPolicy`.

### Tests

Add unit tests for:

- `ExecutionPolicy::from_profile` for frontier, fast/tool-fragile, local, and default profiles.
- Config override of context window and max parallel tools.
- Fast/tool-fragile profile enables explicit tool contract, minimal tool exposure, bootstrap, and post-tool nudge.
- Frontier profile does not require bootstrap or post-tool nudge by default.

Acceptance criteria:

- Existing tests pass.
- `cargo test model_profile` passes.
- `cargo test agent::policy` or equivalent passes.
- No user-facing config breakage.

## Phase 2: improve context tracking and compaction without large rewrites

### Objective

Make context management more model-aware and less lossy. Avoid dumping huge tool outputs into the transcript. Preserve task state and tool-call invariants.

### Files

- `src/agent/compaction.rs`
- `src/agent/loop.rs`
- `src/tool/risk.rs`
- `src/session/events.rs`
- `src/task_state/mod.rs`

### Fix token estimator model usage

`ContextTracker` stores `model: Option<String>`, but most methods currently call `Self::estimate_tokens(...)`, which ignores `self.model`. Change internal estimation in `add_message`, `estimate_tokens_for_messages`, and similar paths to call `estimate_tokens_sync(text, self.model.as_deref())`.

Keep static `ContextTracker::estimate_tokens(text)` for callers that do not know the model.

Add tests:

- Tracker with `with_model(Some("claude..."))` applies multiplier.
- Tracker without model preserves old behavior.

### Add context frame type

Add a minimal structured context frame before attempting any sophisticated memory system.

Possible type in `src/agent/compaction.rs` or new `src/agent/context_frame.rs`:

```rust
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ContextFrame {
    pub user_goal: Option<String>,
    pub current_task: Option<String>,
    pub constraints: Vec<String>,
    pub decisions: Vec<String>,
    pub touched_files: Vec<String>,
    pub commands_run: Vec<String>,
    pub test_results: Vec<String>,
    pub unresolved_errors: Vec<String>,
    pub security_findings: Vec<String>,
    pub next_steps: Vec<String>,
}
```

First implementation can be heuristic and conservative. Do not over-engineer extraction. Populate from:

- Todo state where available.
- Recent tool events and test run events if accessible.
- Recent messages with simple heuristics.
- Security findings already tracked in `AgentLoop`.

Add method:

```rust
impl ContextFrame {
    pub fn to_control_text(&self) -> String { ... }
}
```

The output should be concise and stable, for example:

```text
Current session context:
- Goal: ...
- Active task: ...
- Constraints: ...
- Touched files: ...
- Commands/tests: ...
- Open issues: ...
- Next steps: ...
```

### Use context frame during compaction

Modify `compact_if_needed` so when compaction triggers, the resulting messages preserve:

1. Original system/control messages.
2. A synthetic context-frame control message.
3. Last N raw turns.
4. Valid assistant tool-call / tool-result pairs.

Do not yet attempt perfect summarization. A deterministic frame is better than a vague LLM summary.

Keep existing LLM summarization path behind config. If both exist, prefer deterministic frame plus optional LLM summary only for older narrative context.

### Tool output pruning

Replace direct large result insertion with a token-based cap controlled by `ExecutionPolicy.max_tool_result_tokens`.

In `execute_tool_calls`, after each tool returns:

- If output is below cap, keep as-is.
- If above cap, summarize deterministically if possible.
- Append a clear continuation hint: `Output truncated. Re-run/read narrower path or use tool-specific continuation if available.`

For now, this can be generic. Later phases can add tool-specific continuation handles.

Use safe char boundaries. There is already use of `floor_char_boundary`; keep that pattern.

### Tests

Add tests for:

- Token estimator respects model multiplier.
- Tool output pruning does not split UTF-8.
- Compaction preserves valid assistant tool calls and tool messages.
- Context frame message is inserted when compaction occurs.
- Existing compaction invariants continue to pass.

Acceptance criteria:

- Compaction never creates orphan tool results or assistant tool calls without corresponding results.
- Large outputs are capped before they can dominate context.
- The last active task and touched files remain visible after compaction.

## Phase 3: adaptive tool exposure

### Objective

Expose fewer tools initially, especially for weaker/local/tool-fragile models. Use `tool_search` and the catalog for discovery. Keep full tool availability internally, but do not always send every tool schema to the model.

### Files

- `src/tool/mod.rs`
- `src/tool/catalog.rs`
- `src/tool/tool_search.rs`
- `src/agent/loop.rs`
- `src/model_profile/types.rs`
- `src/model_profile/resolve.rs`

### Implement tool exposure modes

Tool exposure should be based on `ExecutionPolicy.initial_tool_mode`:

`Full`:

- Current behavior, minus disabled tools.
- Appropriate for trusted high-reliability models or explicit user config.

`Curated`:

- Expose common core tools plus task-relevant tools.
- Suggested default for frontier/workhorse hosted models.
- Core set: `read`, `list`, `grep`, `glob`, `codesearch`, `edit`, `apply_patch`, `bash`, `git`, `diff`, `todowrite/todoread` if enabled, `question`, `tool_search`, `skill`.
- Add `websearch`/`webfetch` only when task envelope needs web or config says always expose.
- Add `lsp` only when experimental LSP is enabled and profile/tool budget allows.
- Add `security` when security is enabled or task is security-sensitive.

`MinimalWithDiscovery`:

- Expose only `read`, `list`, `grep` or `codesearch`, `edit` or `apply_patch`, `bash`, `question`, `todowrite/todoread` if enabled, and `tool_search`.
- Hide high-noise tools unless discovered.
- Default for local/tool-fragile/fast executor profiles.

### Respect profile preferred/disabled tools

In `build_tool_definitions`, apply:

1. Permission filters.
2. Plan-mode filters.
3. Profile disabled tools.
4. Exposure-mode selection.
5. Profile preferred tool ordering.
6. MCP tool inclusion policy.

MCP tools should not always be appended wholesale for minimal mode. In minimal mode, expose either no MCP tools or only MCP tools explicitly preferred by profile/config. Keep `tool_search` aware of available hidden tools if safe.

### Improve cache invalidation

Current cache uses MCP tool count as a proxy. Add a stable hash/version for MCP tool identities if available. If not available, compute a local hash over MCP tool names and parameter schemas after `mcp.list_tools()`.

Replace cache key field `mcp_tool_count` with `mcp_tool_hash: u64` or add it alongside count.

### Tests

Add tests for:

- Minimal mode exposes a small known set.
- Curated mode includes core coding tools but excludes unrelated tools.
- Disabled tools are removed even if core/preferred.
- Preferred tools are ordered early.
- MCP cache invalidates when tool names change but count remains the same.

Acceptance criteria:

- Tool schema count drops substantially for local/tool-fragile models.
- Existing full tool mode remains possible by config.
- `tool_search` remains available in minimal/curated modes.

## Phase 4: better task routing with a task envelope

### Objective

Replace fragile keyword-only routing with a small multidimensional task classifier. Keep old `Simple/Medium/Complex` as compatibility output if useful, but derive it from richer task signals.

### Files

- `src/agent/router.rs`
- `src/agent/loop.rs`
- `src/model_profile/types.rs`
- `src/config/schema.rs`

### Add task envelope

In `src/agent/router.rs`, add:

```rust
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TaskEnvelope {
    pub complexity: TaskComplexity,
    pub requires_repo_inspection: bool,
    pub mutation_risk: MutationRisk,
    pub breadth: TaskBreadth,
    pub needs_planning: bool,
    pub needs_tests: bool,
    pub security_sensitive: bool,
    pub likely_long_context: bool,
    pub requested_subagent: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MutationRisk { ReadOnly, Low, Medium, High }

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TaskBreadth { SingleFile, MultiFile, WholeRepo, Unknown }
```

Add `classify_envelope(prompt, active_agent, maybe_tools)`.

Heuristics are fine for now, but they should be explicit and tested:

- Read/list/show/find with no edit intent -> read-only.
- “implement”, “fix”, “modify”, “refactor”, “write file” -> mutation risk.
- “architecture”, “review harness”, “codebase”, “repo”, “subsystems” -> whole-repo/long-context.
- “security”, “vulnerability”, “sandbox”, “permission”, “CVE”, “injection” -> security-sensitive.
- “test”, “failing”, “regression”, “bug” -> needs tests/debug.
- Explicit `@agent` or task tool usage -> requested_subagent if parseable.

### Route with policy matrix

Modify `ModelRouter` so it routes from `TaskEnvelope`, not just prompt/tool keyword.

Suggested behavior:

- Read-only + low breadth -> small model.
- Single-file low-risk edit -> medium model.
- Multi-file implementation -> medium or complex depending config.
- Whole-repo architecture/review/security/debug -> complex model.
- Summaries/compaction/title -> summarizer model if configured.

Preserve existing config keys: `small_model`, `medium_model`, `model`, `auto_route_models`. Add optional future keys only if necessary.

### Integrate with tool exposure and compaction

Pass `TaskEnvelope` into `ExecutionPolicy` creation or keep it alongside policy.

Use it to decide:

- Tool exposure mode additions.
- Whether to expose web/security/LSP tools.
- Whether to enable bootstrap read/list.
- Whether to reserve more context.
- Whether to suggest subagent delegation.

### Tests

Add classifier tests for common prompts:

- “show me src/main.rs” -> read-only, simple, small.
- “fix typo in README” -> low mutation, single-file, medium or small depending config.
- “review the architecture of the coding harness” -> whole-repo, long-context, complex.
- “investigate failing async cancellation tests” -> debug, tests, complex or medium-high.
- “look for prompt injection/security issues” -> security-sensitive, complex.

Acceptance criteria:

- Existing routing behavior remains roughly compatible for simple/medium/complex.
- Architecture/security/repo-review tasks route to complex/frontier model more reliably.
- Small routine operations can still use configured small model.

## Phase 5: subagent delegation contracts

### Objective

Make subagents context-efficient and predictable. Parent should receive a compact typed result, not a long unstructured transcript.

### Files

- `src/agent/worker.rs`
- `src/tool/task.rs`
- `src/agent/mod.rs`
- `src/agent/prompt.rs`
- `src/task_state/*`
- `src/session/events.rs`

### Add subagent result schema

Replace or augment `SubAgentResult { result: String }` with typed metadata while keeping string compatibility.

Suggested structure:

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubAgentReport {
    pub summary: String,
    pub files_examined: Vec<String>,
    pub commands_run: Vec<String>,
    pub findings: Vec<SubAgentFinding>,
    pub next_steps: Vec<String>,
    pub confidence: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubAgentFinding {
    pub severity: Option<String>,
    pub file: Option<String>,
    pub line: Option<u32>,
    pub title: String,
    pub rationale: String,
}
```

Then `SubAgentResult` can become:

```rust
pub struct SubAgentResult {
    pub task_id: u64,
    pub success: bool,
    pub result: String,
    pub report: Option<SubAgentReport>,
}
```

This preserves compatibility.

### Role-specific output contracts

Add prompt fragments for common subagent roles:

- Explore: files examined, symbols/modules, relevant relationships, uncertainties.
- Review: issues by severity, file/line, rationale, suggested patch scope.
- Debug: commands/logs, failure signature, root-cause candidates, next experiment.
- Test: tests added/run, pass/fail, coverage gaps.
- Security: finding category, exploitability, affected surface, mitigation.

Put these in `src/agent/prompts/` or in `prompt.rs` built-ins. Keep them short.

### Enforce budgets

`SubAgentRequest` already has `denied_tools`, `allowed_paths`, and `depth`. Add optional fields if practical:

```rust
pub max_tool_calls: Option<usize>,
pub max_return_tokens: Option<usize>,
pub output_contract: Option<SubAgentOutputContract>,
```

If changing request shape is too invasive, enforce budgets through agent `steps`, tool filtering, and prompt contract first.

### Parent context behavior

When a subagent completes:

- Store full transcript/events in event store if available.
- Return only `SubAgentReport` or a compact textual rendering to parent.
- Do not inject child transcript into parent messages.
- Include pointers/session/task IDs internally if UI wants to inspect later.

### Todo access

Default subagents to no global todo mutation. Use `TaskStatePolicy.subagent_todo_access` to decide if they can read scoped todo state. Avoid letting subagents rewrite the parent task list.

### Tests

Add tests for:

- Subagent report parsing from valid JSON/text if model returns schema.
- Fallback when subagent returns plain text.
- Parent receives compact result only.
- Denied tools are filtered for subagents.
- Depth limit still works.

Acceptance criteria:

- Subagent outputs are bounded and parent-friendly.
- Existing task tool behavior remains compatible.
- Subagents cannot pollute parent context with long transcripts.

## Phase 6: prompt assembly cleanup

### Objective

Reduce duplicated prompt/control-message logic and make prompt injection profile-aware.

### Files

- `src/agent/prompt.rs`
- `src/model_profile/policy.rs`
- `src/agent/loop.rs`

### Work items

1. Keep `select_provider_prompt` but consider routing it through `PromptProfileKind` instead of raw model string. Current string matching can remain as fallback.
2. Move all startup injections into one function that takes `ExecutionPolicy`.
3. Ensure late control messages use `push_control_instruction` consistently.
4. Avoid repeatedly appending duplicate todo/tool-contract text after compaction or resume.
5. Add a simple dedupe mechanism for control instructions, perhaps by marker prefix or enum source.

### Tests

- Tool contract injected only once.
- Todo discipline injected according to task-state policy.
- Late-system-message-averse models merge into first system or use user control messages.
- Frontier models can receive late system messages if policy permits.

Acceptance criteria:

- Prompt assembly behavior is deterministic.
- No repeated control paragraphs after multiple turns.
- Existing custom instructions and AGENTS.md loading still work.

## Phase 7: config and documentation

### Objective

Expose the new behavior without forcing users to understand internals.

### Files

- `src/config/schema.rs`
- `codegg.example.jsonc`
- `docs/ARCHITECTURE.md`
- `architecture/agent.md`
- `README.md` if appropriate

### Config additions

Only add config keys that are immediately used. Prefer under existing `model_profile` entries.

Possible JSONC shape:

```jsonc
{
  "model_profile": {
    "minimax/minimax-2.7": {
      "prompt_profile": "fast_executor",
      "context_window": 128000,
      "max_output_tokens": 8192,
      "max_parallel_tools": 1,
      "requires_explicit_tool_contract": true,
      "requires_post_tool_continue_nudge": true,
      "prefers_small_patches": true,
      "task_state_policy": {
        "mode": "guided_current_task",
        "inject_after_tool_calls": 3
      }
    }
  },
  "compaction": {
    "auto": true,
    "prune": true,
    "threshold": 0.75,
    "reserved": 10000
  }
}
```

Document the recommended profile classes:

- Frontier reasoning.
- Frontier executor.
- Long-context planner.
- Fast executor/tool-fragile.
- Local strict.
- Summarizer.
- Reviewer.

### Documentation updates

Update `architecture/agent.md` after implementation, not before. The doc currently describes the agent loop, compaction, worker/subagent pool, model router, team coordination, and prompt assembly. Keep it accurate.

Add a short section:

- How execution policy is derived.
- How model profiles affect context/tool/todo behavior.
- How tool exposure modes work.
- How subagent result contracts prevent context pollution.

Acceptance criteria:

- Example config validates.
- Architecture docs match code.
- Users can opt into full tool exposure if they dislike adaptive mode.

## Suggested implementation order for a smaller model

Do this in small PR-sized chunks.

### PR 1: ExecutionPolicy skeleton

- Add `ExecutionPolicy` type.
- Build from `ResolvedModelProfile` and config.
- Add tests.
- No behavior change except maybe logging policy values.

### PR 2: Context tracker model awareness

- Fix `ContextTracker` to use `self.model` internally.
- Wire active model into tracker where practical.
- Add tests.

### PR 3: Gate existing heuristics through policy

- Synthetic bootstrap only when policy allows.
- Post-tool continuation nudge only when policy allows.
- Max parallel tools uses profile/policy before server default.
- Reserved output/tool pruning use policy defaults.
- Add tests around policy gates.

### PR 4: Tool exposure modes

- Implement minimal/curated/full filtering.
- Respect preferred/disabled tools.
- Keep full mode as compatibility option.
- Add tests.

### PR 5: Context frame v1

- Add deterministic `ContextFrame`.
- Insert during compaction.
- Preserve invariants.
- Add tests.

### PR 6: TaskEnvelope router

- Add richer classifier.
- Keep old simple/medium/complex output.
- Route based on task envelope.
- Add tests.

### PR 7: Subagent report schema

- Add `SubAgentReport` alongside existing string result.
- Add role-specific output contracts.
- Ensure parent receives compact result.
- Add tests.

### PR 8: Docs and example config

- Update `codegg.example.jsonc`.
- Update architecture docs.
- Add migration notes if any behavior changed.

## Specific implementation notes and guardrails

### Do not overfit to one model vendor

The policy should classify model behavior by capability class, not just exact model names. Exact names are still useful as overrides, but defaults should be broad and conservative.

### Keep config overrides stronger than built-ins

If user config says `max_parallel_tools = 4`, respect it unless there is a safety reason not to. Document any clamping.

### Preserve permission checks

Do not bypass `PermissionChecker`, `SecurityService`, `DoomLoopDetector`, or snapshot capture. Tool exposure controls what the model sees; permissioning still controls what executes.

### Keep tool-call invariants

Any compaction or context-frame insertion must preserve provider message validity:

- No orphan tool result without corresponding assistant tool call unless provider serializer explicitly tolerates it.
- No assistant tool call without required tool result.
- Preserve tool call IDs.
- Preserve assistant/tool ordering.

### Prefer deterministic summaries first

Do not rely on LLM summarization as the only compaction path. Deterministic frames are cheaper, testable, and safer.

### Add tracing

Add concise debug/info logs for:

- Resolved execution policy.
- Tool exposure mode and count.
- Context compaction trigger reason.
- Tool result pruning.
- Router task envelope.
- Subagent report size.

Avoid logging sensitive full tool outputs.

## Manual test checklist

After implementation, manually test these flows:

1. Simple read-only prompt: `show me the structure of src/agent`.
   - Should route to small model if configured.
   - Should expose minimal/curated read/search tools.
   - Should not write todos unless task becomes multi-step.

2. Repo architecture review: `review the coding harness architecture and suggest refinements`.
   - Should classify as whole-repo/long-context/complex.
   - Should route to complex model.
   - Should expose codesearch/read/grep/list and possibly review/security tools.
   - Should compact with a useful context frame if context grows.

3. Tool-fragile model run.
   - Should receive explicit tool contract.
   - Should use minimal tool exposure.
   - Should allow bootstrap list only if model fails to call tools.
   - Should use post-tool nudge only when policy says so.

4. Local model run.
   - Should avoid late system messages.
   - Should use user/control-message-compatible injection.
   - Should cap tool outputs aggressively.
   - Should not receive giant tool schema surfaces.

5. Multi-file edit with tests.
   - Should preserve snapshot behavior.
   - Should run tools with permissions intact.
   - Should summarize test outputs if large.
   - Should keep active task visible after compaction.

6. Subagent exploration.
   - Parent should receive compact report.
   - Child transcript should not flood parent context.
   - Denied tools and path restrictions should hold.

## Definition of done

The work is complete when:

- Execution behavior is primarily derived from `ExecutionPolicy` / model profile rather than ad hoc checks.
- Context tracker uses model-aware estimation.
- Large tool outputs are capped before entering context.
- Compaction creates a useful deterministic context frame.
- Tool exposure can be full, curated, or minimal-with-discovery.
- Router uses a richer task envelope while preserving simple/medium/complex compatibility.
- Subagent outputs have a typed compact report path.
- Tests cover model policy, routing, compaction invariants, tool exposure, and subagent result behavior.
- Architecture docs and example config are updated.



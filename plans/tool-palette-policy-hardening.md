# Tool Palette Policy Hardening Plan

## Purpose

Harden the first active context policy prototype before expanding into any additional active behavior.

The current implementation correctly introduces a gated, conservative tool-palette reduction policy. It defaults off, supports observe/warn/reduce rollout, mutates only `request.tools`, preserves recovery/search/task tools, and avoids transcript/context rewriting.

The main design risk is that reductions are applied to the current `request.tools` list. Since `request.tools` can already be reduced from a prior provider call in the same run, subsequent reductions may compound and permanently lose tools for the rest of the run unless the full profile-filtered tool palette is rebuilt. This pass should make the reducer stateless per provider call by deriving reduced palettes from a full source-of-truth palette each time.

This pass should also add starvation/backoff signals and clearer diagnostics showing full palette count -> selected count from the unreduced base palette.

## Current state summary

Relevant files:

- `src/context/policy.rs`
  - `ContextPolicyDecisionKind::{Noop, WarnOnly, ReduceToolPalette}`.
  - `ContextPolicyDecision`.
  - `ToolPaletteReduction`.
  - `decide_policy(...)`.
  - `reduce_tool_palette(...)`.
  - Tests for gating, required tool preservation, cap overflow, deterministic selection, tool-search preservation, empty fallback, extra required names, `never_reduce_tools`.
- `src/agent/loop.rs`
  - Loads `context_policy_config`.
  - Calls `apply_tool_palette_policy_if_active(&mut request, "InitialRequest")` after model-profile filtering.
  - Calls `apply_tool_palette_policy_if_active(&mut request, "BeforeProviderCall")` before the `BeforeProviderCall` observation.
  - Helper mutates `request.tools` only.
- `crates/codegg-config/src/schema.rs`
  - Adds `[context_policy]` with safe defaults.
- `architecture/cache-aware-context.md`, `AGENTS.md`, `.opencode/skills/context/SKILL.md`, `README.md`
  - Updated docs.

## Non-goals

Do not enable active context/message compaction.

Do not rewrite transcript messages.

Do not remove tools from `ToolRegistry`.

Do not introduce semantic/vector tool selection.

Do not persist policy state to SQLite.

Do not make reduction default-on.

Do not make dynamic subagent/tool-specific routing changes in this pass.

## Phase 1: Preserve a full per-turn tool palette source of truth

Add an `AgentLoop` field to preserve the full profile-filtered palette for the current run/turn:

```rust
full_tool_palette: Vec<crate::provider::ToolDefinition>,
```

or, if avoiding a struct field, maintain a local `base_tool_palette` in `run()` and pass it to the policy helper.

Preferred field naming:

```rust
base_request_tools: Vec<crate::provider::ToolDefinition>,
```

Requirements:

- Build full tool definitions as currently done:

```rust
let filtered_tools = crate::agent::policy::filter_tool_definitions_for_profile(
    self.build_tool_definitions().await,
    &model_profile,
);
```

- Store/clone this as the unreduced base palette.
- Set `request.tools` from the base palette after applying policy.
- On every provider call, recompute selected tools from `base_request_tools`, not from the current possibly-reduced `request.tools`.
- If model-profile policy or execution policy changes mid-run, explicitly rebuild the base palette.

Acceptance:

- A tool omitted by policy in one provider call can reappear in a later call if policy noops, backoff disables reduction, or config/state changes.
- Reduction is no longer cumulative across loop iterations.

## Phase 2: Refactor policy helper to take base palette explicitly

Change:

```rust
fn apply_tool_palette_policy_if_active(&mut self, request: &mut ChatRequest, phase: &str)
```

into something like:

```rust
fn apply_tool_palette_policy_if_active(
    &mut self,
    request: &mut ChatRequest,
    phase: &str,
    base_tools: &[crate::provider::ToolDefinition],
)
```

Behavior:

- Start from `base_tools` every time.
- If policy disabled/noop/warn, set or leave `request.tools = Some(base_tools.to_vec())` unless `request.tools` is intentionally `None` due to max-steps termination or tool-disabled mode.
- If policy reduces, set `request.tools = Some(reduction.selected)`.
- If `request.tools` is `None` for a legitimate control reason, do not re-enable tools.

Important:

- The helper should not inspect `request.tools` as its source of truth except to respect `None`.
- `current_tool_count` should be `base_tools.len()`, not the already-reduced count.
- Diagnostics should report base count, selected count, and omitted count from the base palette.

Tests:

- Repeated reduction from the same base palette produces the same selected set.
- If a second call noops after a prior reduction, request tools are restored to the full base palette.
- If request tools are `None`, helper does not re-enable tools.

## Phase 3: Add policy runtime state for starvation/backoff

Add minimal in-memory policy state to `AgentLoop`:

```rust
struct ContextPolicyRuntimeState {
    reduction_disabled_until_turn: Option<usize>,
    consecutive_reductions: usize,
    last_selected_tool_count: usize,
    last_omitted_tools: Vec<String>,
    last_reason: Option<String>,
}
```

or a simpler equivalent.

Backoff rules for this pass:

- If selected tool palette would be empty, fall back to base palette and set `reduction_disabled_until_turn = current_turn + 1`.
- If cap is exceeded by required tools, allow required overflow but log it.
- If the provider returns/attempts a tool call that was omitted from the current selected palette but exists in the base palette, treat this as a starvation signal and disable reduction for the next provider call.
- If the assistant text indicates missing tools with obvious phrases like `tool not available`, `cannot access tool`, `missing tool`, add a warning only. Do not attempt brittle broad NLP.

Do not overbuild dynamic recovery. This is a safety valve, not a full planner.

Acceptance:

- A clear starvation signal disables reduction for at least the next provider call.
- Backoff is in-memory and resets between sessions/runs.

## Phase 4: Detect omitted-tool call attempts

After provider events are processed and `tool_calls` are parsed, compare tool call names against the last selected palette and base palette.

Pseudo:

```rust
let selected_names: HashSet<_> = self.context_policy_runtime.last_selected_tools.iter().cloned().collect();
let base_names: HashSet<_> = base_tools.iter().map(|t| t.name.clone()).collect();

for tc in &tool_calls {
    if base_names.contains(tc.name.as_ref()) && !selected_names.contains(tc.name.as_ref()) {
        tracing::warn!(
            tool = %tc.name,
            "context policy starvation: model attempted omitted tool; disabling reduction for next turn"
        );
        self.context_policy_runtime.reduction_disabled_until_turn = Some(self.state.turn_count + 1);
    }
}
```

Notes:

- If the model calls a tool not in base palette at all, that is not caused by policy reduction; do not treat it as a starvation signal.
- If selected palette is not reduced for this call, do nothing.
- This should happen before tool execution where possible, but do not fail the tool call because of this signal.

Tests:

- Omitted-but-base-present tool call sets backoff.
- Tool not in base does not set backoff.
- Tool present in selected palette does not set backoff.

## Phase 5: Improve diagnostics

Current logs are good but should make the source-of-truth distinction explicit.

Add fields:

```rust
base_tool_count = base_tools.len(),
selected_tool_count = selected.len(),
omitted_tool_count = omitted.len(),
policy_backoff_active = bool,
reduction_disabled_until_turn = ?state.reduction_disabled_until_turn,
cap_exceeded_by_required = red.cap_exceeded_by_required,
```

At debug level, log:

- selected tool names,
- omitted tool names,
- required tool names that forced cap overflow.

In warn mode, log:

- `would_reduce_from = base_tools.len()`
- `would_select = reduction.selected.len()` if a dry-run reduction is cheap to compute.

Acceptance:

- Logs can answer whether the full palette was 60 tools and the selected palette was 24.
- Logs identify when backoff prevented reduction.

## Phase 6: Add dry-run reduction for Warn mode

Warn mode currently emits a warning decision but may not compute the actual reduced palette.

For better observability:

- In `Warn` mode when `ReviewToolPalette` would trigger, call `reduce_tool_palette(base_tools, ...)` as a dry run.
- Do not mutate `request.tools`.
- Log would-select/would-omit counts and debug names.

Tests:

- Warn mode does not mutate request tools.
- Warn mode logs/returns decision with would-selected and would-omitted counts if captured in the decision model.

Implementation option:

- Extend `ContextPolicyDecision` to include optional `would_selected_tool_count` and `would_omitted_tool_count`, or keep it only in logging. Prefer extending decision if tests need it.

## Phase 7: Make `review_tool_palette_threshold` effective

`ContextPolicyConfig` has `review_tool_palette_threshold`, but `decide_policy()` should explicitly use it.

Currently the policy should only use `ReviewToolPalette` as a trigger if:

```rust
config.review_tool_palette_threshold()
```

is true.

Update `decide_policy()`:

```rust
let is_review = config.review_tool_palette_threshold()
    && analysis.recommended_action == EffectiveCostAction::ReviewToolPalette;
```

Tests:

- With `review_tool_palette_threshold=false`, warn/reduce do not trigger on `ReviewToolPalette`.

## Phase 8: Integration tests / targeted unit tests

Add tests where feasible:

### Base palette restoration

1. Base tools: 30.
2. Reduce active with cap 10 -> request tools 10.
3. Disable policy or trigger backoff.
4. Apply helper again with the same base tools.
5. Request tools return to 30.

### No cumulative shrink

1. Base tools: 30.
2. Reduce cap 10.
3. Apply again from same base.
4. Still 10, not fewer.

### Request tools `None` respected

1. `request.tools = None` due to max-step/termination.
2. Apply helper with base tools.
3. Remains `None`.

### Omitted tool starvation

1. Base includes `bash`.
2. Selected omits `bash`.
3. Model attempts `bash`.
4. Backoff activates for next call.

### Warn dry-run nonmutation

1. Warn mode enabled.
2. Trigger `ReviewToolPalette`.
3. Request tools unchanged.
4. Would-reduce counts available/loggable.

### Threshold disabled

1. `review_tool_palette_threshold=false`.
2. `ReviewToolPalette` analysis.
3. No warn/reduce.

## Phase 9: Docs

Update:

- `architecture/cache-aware-context.md`
- `AGENTS.md`
- `.opencode/skills/context/SKILL.md`

Document:

- Reduction is derived from a full base palette every provider call.
- Reduction is not cumulative.
- Backoff/starvation handling.
- Warn mode does dry-run counts.
- `review_tool_palette_threshold=false` disables use of that effective-cost action as a trigger.
- Defaults remain disabled.

## Phase 10: Validation

Run:

```bash
cargo fmt
cargo clippy --workspace --all-targets --all-features
cargo test --workspace --all-features context_policy
cargo test --workspace --all-features context
cargo test --workspace --all-features
```

If full workspace tests are impractical or unrelated failures exist, document exact commands and failures.

## Acceptance criteria

This pass is complete when:

1. Tool reduction is derived from an unreduced base palette every provider call.
2. Reduction is not cumulative across loop iterations.
3. Noop/backoff can restore the full base palette.
4. `request.tools = None` is respected and not accidentally re-enabled.
5. Starvation signals disable reduction for at least one subsequent provider call.
6. Warn mode can report dry-run selected/omitted counts without mutation.
7. `review_tool_palette_threshold=false` prevents ReviewToolPalette-triggered warn/reduce.
8. Diagnostics clearly show base count, selected count, omitted count, cap overflow, and backoff status.
9. Docs describe source-of-truth palette and backoff semantics.
10. Formatting, clippy, and targeted tests pass.

## Suggested stopping point

Stop when the active tool-palette policy is safe to run in warn mode and low-risk reduce mode without cumulative shrinking or starvation surprises.

Only after this should the project consider a separate volatile-tail compaction prototype, still behind a strict gate and with no system-prompt rewriting.

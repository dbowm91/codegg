# Tool Palette Starvation Verification Plan

## Purpose

Verify and, if needed, complete the starvation/backoff wiring for the active tool-palette context policy.

The current hardening pass appears to have fixed the largest policy issue: reductions are now derived from `base_request_tools` rather than the currently reduced `request.tools`, so reduction should be non-cumulative and restorable. Runtime state and backoff fields also exist.

The remaining uncertainty is whether the promised starvation signal is fully wired and tested: when the model attempts a tool that exists in the base palette but was omitted by the reduced selected palette, the policy should disable reduction for at least the next provider call.

This plan is intentionally narrow. Do not add new policy modes or broader active compaction.

## Current state summary

Relevant files:

- `src/agent/loop.rs`
  - `base_request_tools: Vec<ToolDefinition>` exists as the full profile-filtered source-of-truth palette.
  - `ContextPolicyRuntimeState` exists with:
    - `reduction_disabled_until_turn`
    - `consecutive_reductions`
    - `last_selected_tool_count`
    - `last_omitted_tools`
    - `last_reason`
    - `last_selected_tools`
  - `apply_tool_palette_policy_if_active(...)` derives reduction from `base_request_tools`, respects `request.tools=None`, applies backoff, and logs richer diagnostics.
  - Tool calls are parsed after provider events are processed.
- `src/context/policy.rs`
  - `decide_policy(...)` and `reduce_tool_palette(...)` exist.
  - Tests cover policy gating and reducer behavior.
- `crates/codegg-config/src/schema.rs`
  - `[context_policy]` config and `review_tool_palette_threshold` exist.

## Non-goals

Do not change default behavior.

Do not enable active compaction.

Do not rewrite transcript messages.

Do not mutate `ToolRegistry`.

Do not add semantic/vector tool selection.

Do not persist policy runtime state.

Do not treat arbitrary unknown tool calls as starvation caused by policy reduction.

## Phase 1: Locate or add a dedicated starvation helper

Search for existing starvation logic around parsed tool calls. Look for terms such as:

- `starvation`
- `last_selected_tools`
- `last_omitted_tools`
- `reduction_disabled_until_turn`
- `base_request_tools`
- `omitted tool`

If logic exists inline, consider extracting it into a dedicated helper for testability:

```rust
fn observe_tool_palette_starvation(&mut self, tool_calls: &[ToolCall]) -> bool
```

Suggested behavior:

```rust
fn observe_tool_palette_starvation(&mut self, tool_calls: &[ToolCall]) -> bool {
    if self.base_request_tools.is_empty() {
        return false;
    }
    if self.context_policy_runtime.last_selected_tools.is_empty() {
        return false;
    }
    if self.context_policy_runtime.last_omitted_tools.is_empty() {
        return false;
    }

    let base_names: HashSet<&str> = self
        .base_request_tools
        .iter()
        .map(|t| t.name.as_str())
        .collect();
    let selected_names: HashSet<&str> = self
        .context_policy_runtime
        .last_selected_tools
        .iter()
        .map(|s| s.as_str())
        .collect();

    let mut triggered = false;
    for tc in tool_calls {
        let name = tc.name.as_ref();
        if base_names.contains(name) && !selected_names.contains(name) {
            triggered = true;
            tracing::warn!(
                policy = "context_tool_palette",
                tool = %name,
                turn_count = self.state.turn_count,
                disabled_until_turn = self.state.turn_count + 1,
                "context policy starvation: model attempted omitted base-palette tool; disabling reduction for next provider call"
            );
        }
    }

    if triggered {
        self.context_policy_runtime.reduction_disabled_until_turn = Some(self.state.turn_count + 1);
        self.context_policy_runtime.last_reason = Some(
            "starvation: model attempted omitted base-palette tool".to_string(),
        );
    }

    triggered
}
```

Important distinctions:

- If the tool name is not in `base_request_tools`, do not treat it as policy starvation.
- If the tool name is in `last_selected_tools`, do not treat it as starvation.
- If no reduction has occurred (`last_selected_tools` or `last_omitted_tools` empty), do nothing.
- Do not block the tool call solely because of this signal; this pass only controls future reduction.

## Phase 2: Wire the helper immediately after tool-call parsing

The helper should run after the final `tool_calls` list is known, including text-parsed fallback tool calls, and before tool execution where practical.

Current rough flow:

1. Provider events are streamed.
2. Events are processed into `EventProcessor`.
3. Usage is recorded from `processor`.
4. `tool_calls = processor.tool_calls().to_vec()`.
5. If empty, parse tool calls from text fallback.
6. Tool calls are executed or bootstrap logic runs.

Add the starvation observation after step 5, before any execution branch:

```rust
let starvation_triggered = self.observe_tool_palette_starvation(&tool_calls);
```

If bootstrap synthetic tools are generated later, do not treat them as starvation unless they were requested by the model. Bootstrap is an agent fallback, not model evidence.

If there are multiple loops / follow-up drain paths that parse tool calls separately, wire the helper in each path or document why only the main path is relevant.

Acceptance:

- Parsed structured tool calls and text-fallback tool calls both feed the starvation detector.
- Synthetic bootstrap calls do not trigger starvation.

## Phase 3: Ensure backoff takes effect on next provider call

`apply_tool_palette_policy_if_active(...)` already appears to check `reduction_disabled_until_turn` and restore `base_request_tools` when backoff is active.

Verify and test that after starvation:

1. `reduction_disabled_until_turn = Some(current_turn + 1)`.
2. The next call to `apply_tool_palette_policy_if_active(...)` sees backoff active.
3. `request.tools` is restored to `base_request_tools`.
4. No reduction occurs for that call.
5. Once `state.turn_count > disabled_until_turn`, reduction may resume if policy conditions still hold.

If the turn-count comparison is ambiguous, keep the existing behavior but document it in tests.

Tests:

- At turn `5`, starvation sets disabled-until `6`.
- Applying policy at turn `5` or `6` restores base palette.
- Applying policy at turn `7` can reduce again if all gates trigger.

## Phase 4: Add targeted unit tests

Add tests at the smallest feasible layer.

If full `AgentLoop` construction is heavy, extract pure helpers into `src/context/policy.rs` for easier testing:

```rust
pub fn detect_palette_starvation(
    base_tool_names: impl IntoIterator<Item = String>,
    selected_tool_names: impl IntoIterator<Item = String>,
    called_tool_names: impl IntoIterator<Item = String>,
) -> Vec<String>
```

Then have `AgentLoop::observe_tool_palette_starvation()` call the pure helper.

Required tests:

### Omitted base tool triggers starvation

- base: `read`, `bash`, `edit`
- selected: `read`, `edit`
- called: `bash`
- result: `bash` starvation detected

### Selected tool does not trigger starvation

- base: `read`, `bash`, `edit`
- selected: `read`, `edit`
- called: `read`
- result: none

### Unknown tool does not trigger starvation

- base: `read`, `edit`
- selected: `read`
- called: `nonexistent_tool`
- result: none

### No prior reduction does not trigger starvation

- base: `read`, `bash`
- selected empty or omitted empty
- called: `bash`
- result: none

### Multiple omitted calls are reported deterministically

- base: `read`, `bash`, `grep`, `edit`
- selected: `read`
- called: `grep`, `bash`, `grep`
- result: stable de-duplicated list, preferably in call order: `grep`, `bash`

### Backoff state is set

If testing `AgentLoop` helper directly:

- Configure runtime last selected/omitted.
- Call helper with an omitted base tool.
- Assert `reduction_disabled_until_turn == Some(turn_count + 1)`.
- Assert `last_reason` mentions starvation.

## Phase 5: Improve starvation diagnostics

When starvation occurs, log fields sufficient to debug the policy:

```rust
tracing::warn!(
    policy = "context_tool_palette",
    tool = %tool_name,
    base_tool_count = self.base_request_tools.len(),
    last_selected_tool_count = self.context_policy_runtime.last_selected_tool_count,
    last_omitted_tool_count = self.context_policy_runtime.last_omitted_tools.len(),
    turn_count = self.state.turn_count,
    reduction_disabled_until_turn = ?self.context_policy_runtime.reduction_disabled_until_turn,
    "context policy starvation detected"
);
```

At debug level, log the last selected and omitted names.

Do not log full tool schemas.

## Phase 6: Verify `review_tool_palette_threshold` test coverage

The prior hardening commit claims `decide_policy()` now gates `ReviewToolPalette` on `review_tool_palette_threshold()`.

Verify there is a test for:

- `review_tool_palette_threshold=false`
- analysis recommends `ReviewToolPalette`
- mode is `Warn` or `ToolPaletteReduce`
- decision is `Noop`

If missing, add it.

## Phase 7: Documentation update

Update docs only if the current docs do not clearly state starvation semantics.

Files:

- `architecture/cache-aware-context.md`
- `AGENTS.md`
- `.opencode/skills/context/SKILL.md`

Document:

- Starvation is detected only when the model attempts a tool present in the unreduced base palette but omitted from the selected palette.
- Unknown tools are not blamed on policy reduction.
- Starvation disables reduction for at least the next provider call.
- The tool call is not denied solely because of this signal.

## Phase 8: Validation

Run:

```bash
cargo fmt
cargo clippy --workspace --all-targets --all-features
cargo test --workspace --all-features context_policy
cargo test --workspace --all-features context
cargo test --workspace --all-features
```

If full workspace tests are too expensive or unrelated failures exist, document exact commands and failures.

## Acceptance criteria

This pass is complete when:

1. Starvation detection is either verified as already wired or implemented via a dedicated helper.
2. The detector runs after all model-originated tool-call parsing and before execution where practical.
3. Omitted base-palette tool calls activate reduction backoff.
4. Selected tools and unknown tools do not activate backoff.
5. Synthetic bootstrap tool calls do not activate starvation.
6. Backoff causes the next provider call to use the full base palette.
7. Starvation logic has targeted tests.
8. `review_tool_palette_threshold=false` has explicit test coverage.
9. Diagnostics are sufficient to identify the omitted tool and policy state.
10. Formatting, clippy, and targeted tests pass.

## Suggested stopping point

Stop after starvation/backoff is proven with tests. Do not add new active policy behavior in this pass.

After this, the tool-palette policy is suitable for practical warn-mode trials and carefully controlled reduce-mode trials before considering any volatile-tail context compaction prototype.

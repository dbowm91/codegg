# Volatile Tail Compaction Prototype Plan

## Purpose

Introduce the next active context-policy phase: a strictly gated, late-context-only volatile-tail compaction prototype.

The repo now has the required safety foundation:

- Artifact-backed tool-output projection and `context_read` recovery.
- Stable context block / packer diagnostics.
- Provider finish usage wired into `ContextCacheStats`.
- Effective-cost analysis with `CompactVolatileTailFirst` / `ReviewToolPalette` / `PreserveStablePrefix` recommendations.
- A first active policy, tool-palette reduction, that is gated, base-palette derived, non-cumulative, and guarded by starvation/backoff detection.

This pass should not touch stable prefixes, system prompts, project instructions, tool schemas, model profile text, or early conversation history. It should only prototype active compaction for late volatile context after existing artifact projection and existing compaction mechanisms have already had a chance to operate.

## Current state summary

Relevant files:

- `src/agent/loop.rs`
  - Main provider loop.
  - Existing `compact_if_needed(...)` path.
  - `observe_context_pack(...)` and effective-cost diagnostics.
  - `context_cache_stats` updated from provider finish events.
  - Active tool-palette policy already gated through `[context_policy]`.
- `src/agent/compaction.rs`
  - Existing `ContextTracker`, compaction policies, hybrid/model/programmatic compaction paths.
- `src/context/effective_cost.rs`
  - Emits `EffectiveCostAction::CompactVolatileTailFirst` when volatile context is high and cache hit is poor/unknown.
- `src/context/packer.rs`
  - Computes stable/slow/volatile token estimates and omitted blocks.
- `src/context/block.rs`
  - Classifies blocks by `CacheClass` and `ContextBlockKind`.
- `src/context/policy.rs`
  - Existing policy decision machinery for tool-palette reduction.
- `crates/codegg-config/src/schema.rs`
  - Existing `[context_policy]` and `[context_packer]` config.

## Non-goals

Do not rewrite system prompts.

Do not reorder transcript messages.

Do not compact stable-prefix or slow-changing context.

Do not alter tool definitions or tool-palette policy behavior in this pass.

Do not remove artifact recovery handles.

Do not replace existing compaction modes.

Do not implement semantic/vector retrieval.

Do not make active compaction default-on.

Do not add provider-dollar pricing logic.

## Design principle

This phase should be an overlay policy that says:

> If effective-cost diagnostics say the volatile tail is dominating uncached context, compact only the oldest safe volatile tail material, preserve the recent working tail, and preserve recovery handles.

This is not a full memory system. It is a bounded, reversible, late-context pressure valve.

## Phase 1: Add explicit volatile-tail policy config

Extend `[context_policy]` rather than adding an unrelated config section.

Suggested fields:

```rust
pub volatile_tail_compaction: Option<bool>,
pub volatile_tail_mode: Option<VolatileTailPolicyMode>, // observe | warn | compact
pub min_volatile_tokens_for_compaction: Option<usize>,
pub preserve_recent_messages: Option<usize>,
pub max_compacted_tail_tokens: Option<usize>,
pub require_effective_cost_signal: Option<bool>,
pub compact_tool_results_only_first: Option<bool>,
```

Suggested enum:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum VolatileTailPolicyMode {
    #[default]
    Observe,
    Warn,
    Compact,
}
```

Defaults:

- `volatile_tail_compaction = false`
- `volatile_tail_mode = Observe`
- `min_volatile_tokens_for_compaction = 12000`
- `preserve_recent_messages = 12`
- `max_compacted_tail_tokens = 8000`
- `require_effective_cost_signal = true`
- `compact_tool_results_only_first = true`

Acceptance:

- Existing configs continue to deserialize.
- Defaults do not mutate messages.
- Tool-palette policy defaults remain unchanged.

## Phase 2: Add volatile-tail candidate analysis

Create a module such as:

```text
src/context/volatile_tail.rs
```

Suggested types:

```rust
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VolatileTailCandidateKind {
    ToolResult,
    AssistantNarration,
    UserMessage,
    ControlInstruction,
}

#[derive(Debug, Clone)]
pub struct VolatileTailCandidate {
    pub message_index: usize,
    pub kind: VolatileTailCandidateKind,
    pub estimated_tokens: usize,
    pub has_recovery_handle: bool,
    pub safe_to_compact: bool,
    pub reason: String,
}

#[derive(Debug, Clone)]
pub struct VolatileTailAnalysis {
    pub total_volatile_tail_tokens: usize,
    pub candidate_tokens: usize,
    pub preserved_recent_messages: usize,
    pub candidates: Vec<VolatileTailCandidate>,
}
```

Candidate rules:

- Only inspect messages after the stable/system prefix.
- Preserve the last `preserve_recent_messages` transcript messages.
- Prefer older `Message::Tool` entries because projected tool outputs already have artifact handles when artifact store is enabled.
- Treat messages with recovery handles as safer to compact.
- Do not compact user messages in the first implementation unless they are clearly machine-generated control reminders.
- Do not compact assistant messages with tool calls attached.
- Do not compact messages required by provider message-contract repair.

Acceptance:

- Candidate analysis is pure and non-mutating.
- It exposes what would be compacted before any mutation exists.

## Phase 3: Add a compacted message representation

Do not delete old information without a breadcrumb.

For compacted tool-result messages, replace content with a compact tombstone that preserves:

- original role/message type,
- tool call id,
- recovery handle if present,
- estimated original token count,
- short reason,
- instruction to use `context_read` if full output is needed.

Example replacement for a `Message::Tool` content:

```text
[compacted volatile tool result]
original_estimated_tokens=4312
reason=older volatile tail compacted by context policy
recovery_handle=ctx://tool/<session>/<turn>/<tool_call_id>
Use context_read with the recovery_handle if full output is needed.
```

If no recovery handle exists, either skip compaction by default or produce a more conservative tombstone only when `lossless_debug` / artifact store guarantees exist elsewhere. Preferred first pass: skip no-handle tool results unless explicitly configured later.

Acceptance:

- Compaction is not silent.
- The model sees a clear breadcrumb and recovery path.

## Phase 4: Add decision logic separate from tool-palette policy

Do not overload `ContextPolicyDecisionKind::ReduceToolPalette`.

Add a separate decision type:

```rust
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VolatileTailDecisionKind {
    Noop,
    WarnOnly,
    Compact,
}

#[derive(Debug, Clone)]
pub struct VolatileTailDecision {
    pub kind: VolatileTailDecisionKind,
    pub reason: String,
    pub recommended_action: EffectiveCostAction,
    pub candidate_count: usize,
    pub candidate_tokens: usize,
    pub planned_compaction_tokens: usize,
}
```

Trigger conditions for compact mode:

- config enables volatile tail compaction,
- mode is `Compact`,
- effective-cost action is `CompactVolatileTailFirst` when `require_effective_cost_signal=true`,
- candidate tokens exceed `min_volatile_tokens_for_compaction`,
- at least one candidate has a recovery handle,
- compaction budget does not exceed `max_compacted_tail_tokens`.

Warn mode:

- Computes candidates and planned token savings.
- Logs what would be compacted.
- Does not mutate messages.

Acceptance:

- Tool-palette decisions and volatile-tail compaction decisions are independent.
- Warn mode provides useful dry-run numbers.

## Phase 5: Implement a pure compaction planner

Add a pure function:

```rust
pub fn plan_volatile_tail_compaction(
    messages: &[Message],
    analysis: &EffectiveCostAnalysis,
    config: &ContextPolicyConfig,
) -> VolatileTailPlan
```

or keep provider `Message` types out of `context` by defining an adapter in `agent/loop.rs` and a pure data model in `context`.

The planner should:

1. Identify candidates.
2. Sort candidates by oldest first.
3. Prefer `ToolResult` candidates with recovery handles.
4. Stop when `max_compacted_tail_tokens` would be exceeded.
5. Return candidate indices and planned replacements.

Do not mutate in the planner.

Tests:

- Recent messages are preserved.
- Tool results with handles are selected first.
- No-handle tool results are skipped by default.
- User messages are preserved.
- Assistant messages with tool calls are preserved.
- Budget cap is respected.

## Phase 6: Wire in warn mode first

Before adding mutation, wire the planner into the main loop as warn/observe diagnostics only.

Call site should be after existing tool-output projection and after existing `compact_if_needed(...)`, but before the next provider call:

```rust
self.observe_or_apply_volatile_tail_policy(&mut request.messages, &model_profile, "BeforeProviderCall");
```

In `Observe` mode:

- no logs beyond existing packer logs unless diagnostics enabled.

In `Warn` mode:

- log candidate count,
- candidate tokens,
- planned compaction tokens,
- preserved recent count,
- top candidate kinds/indices.

Acceptance:

- Warn mode can run without mutation.
- Logs show whether the future compact mode would do anything.

## Phase 7: Add compact mode behind the strict gate

Once warn mode planner is tested, implement actual mutation only for `VolatileTailPolicyMode::Compact`.

Mutation constraints:

- Only mutate `Vec<Message>` content for selected `Message::Tool` entries.
- Preserve `tool_call_id` exactly.
- Preserve message order exactly.
- Preserve total message count initially; do not delete messages in this first pass.
- Do not compact assistant messages or user messages in this first pass.
- Do not compact any message lacking a recovery handle.

After mutation:

- Call `context_tracker` update if required by its state model. If it cannot safely be updated incrementally, perform compaction before adding messages to the tracker or rebuild tracker state carefully. Do not leave tracker and request messages badly divergent.

Acceptance:

- Message order and count are unchanged.
- Tool message contracts remain valid.
- Compacted tool messages are visibly recoverable.

## Phase 8: Prevent repeated compaction of the same message

Add a marker check:

```text
[compacted volatile tool result]
```

If a message already starts with that marker, do not compact it again.

Optionally track compacted tool call IDs in a runtime set:

```rust
compacted_volatile_tool_call_ids: HashSet<String>
```

Acceptance:

- Repeated policy application is idempotent.
- Already-compacted messages do not shrink into nested tombstones.

## Phase 9: Diagnostics and events

Add structured logs:

```rust
tracing::info!(
    policy = "volatile_tail_compaction",
    mode = ?mode,
    action = ?decision.kind,
    recommended_action = ?analysis.recommended_action,
    candidate_count = plan.candidates.len(),
    planned_compaction_tokens = plan.planned_tokens,
    applied_compactions = applied_count,
    preserved_recent_messages = config.preserve_recent_messages(),
    "volatile tail policy decision"
);
```

At debug level, log selected message indices, kinds, token estimates, and recovery handles. Do not log full message contents.

Optional event bus integration can be deferred unless there is an existing context-policy event pattern.

## Phase 10: Tests

Add targeted unit tests for planner and replacement formatting.

Required tests:

### Planner preserves stable/recent context

- Given a mixed transcript with system + older tool results + recent messages.
- Planner preserves system and last N messages.

### Planner selects old tool results with handles

- Older `Message::Tool` entries containing `ctx://tool/...` handles are selected first.

### Planner skips no-handle tool results by default

- Tool result lacking handle is not selected.

### Planner preserves user messages

- User messages are not selected in first pass.

### Planner preserves assistant tool-call messages

- Assistant messages carrying tool calls are not selected.

### Budget cap respected

- Planned token count does not exceed `max_compacted_tail_tokens`.

### Tombstone preserves contract

- Replacement keeps `Message::Tool { tool_call_id, content }` shape.
- `tool_call_id` unchanged.
- Content includes recovery handle and marker.

### Idempotence

- Already compacted message is not selected again.

### Warn mode nonmutation

- Warn mode produces a plan/loggable decision but leaves messages unchanged.

### Compact mode mutation scope

- Compact mode mutates only selected tool message contents.
- Message order and count unchanged.

## Phase 11: Documentation

Update:

- `architecture/cache-aware-context.md`
- `AGENTS.md`
- `.opencode/skills/context/SKILL.md`
- README context section if currently tracking the policy rollout

Document:

- This is a volatile-tail-only policy.
- Defaults are disabled.
- Rollout is observe -> warn -> compact.
- Stable prefix is never rewritten.
- Message order/count are preserved in first pass.
- Tool result compaction requires recovery handles.
- Use `context_read` for full recovery.
- Repeated compaction is idempotent.

## Phase 12: Validation

Run:

```bash
cargo fmt
cargo clippy --workspace --all-targets --all-features
cargo test --workspace --all-features volatile_tail
cargo test --workspace --all-features context_policy
cargo test --workspace --all-features context
cargo test --workspace --all-features
```

If full workspace tests are impractical or unrelated failures exist, document exact commands and failures.

## Acceptance criteria

This pass is complete when:

1. Volatile-tail compaction config exists and defaults to no mutation.
2. Planner can identify old volatile tool-result candidates with recovery handles.
3. Planner preserves system/stable prefix, user messages, assistant tool-call messages, and recent tail.
4. Warn mode produces dry-run diagnostics without mutation.
5. Compact mode mutates only selected `Message::Tool` contents behind an explicit gate.
6. Message order, message count, and tool call IDs are preserved.
7. Replacement tombstones include recovery handles and `context_read` guidance.
8. No-handle tool results are skipped by default.
9. Repeated application is idempotent.
10. Tests cover planner, warn mode, compact mode, tombstone formatting, and idempotence.
11. Docs explain rollout and guarantees.
12. Formatting, clippy, and targeted tests pass.

## Suggested stopping point

Stop after volatile-tail compaction is implemented but disabled by default, with warn mode validated and compact mode restricted to old tool-result messages with recovery handles.

Do not expand to user-message or assistant-message summarization until the tool-result-only compaction path has been exercised in real sessions.

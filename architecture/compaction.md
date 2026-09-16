# Compaction Module

## Purpose

The compaction module manages context window overflow by reducing
conversation history while preserving tool call/output invariants and
session state. The canonical production owner is `src/context/compaction.rs`;
the former `src/agent/compaction.rs` path is only a compatibility re-export.
It supports a legacy strategy-based path (truncate, summarize, drop-middle)
and a newer hybrid engine with programmatic evidence extraction, optional
LLM-based semantic enrichment, and invariant validation with emergency
fallback.

## Where It Lives

| File | Role |
|------|------|
| `src/context/compaction.rs` | `ContextTracker`, capacity policy, all compaction strategies, hybrid engine, invariant validation, typed outcomes |
| `src/agent/compaction.rs` | Compatibility re-export only; no production implementation |
| `src/agent/context_frame.rs` | `ContextFrame`, `ContextLedgerState` — post-compaction context snapshot |
| `src/agent/context_runtime.rs` | `compact_if_needed()` — integration point called each turn |
| `src/config/schema.rs` | `CompactionConfig`, `CompactionModeConfig`, `CompactionPolicyConfig` |
| `tests/compaction.rs` | Module-level integration tests |

## How It Works

### Canonical entry point and two strategies

`AgentLoop::compact_if_needed()` is sequencing glue. It invokes
`context::compaction::compact_context(ContextCompactionRequest)` and consumes
the typed `ContextCompactionResult`; hooks, context-frame injection, and
events remain at the agent boundary.

1. **Legacy strategy path** (no `compaction.mode` in config): Uses
   `auto_compact_async()` / `auto_compact_sync()` with strategy
   selection (TruncateToolOutputs, SummarizeOldTurns, DropMiddleMessages).
   Entry: `compact_if_needed()` → `detect_overflow()` → `prune_tool_outputs()`
   → `auto_compact_async()`.

2. **New hybrid engine** (`compaction.mode` set in config): Uses
   `compact_with_policy()` as the primary entry. Builds programmatic
   evidence, applies mode (Programmatic/Agent/Hybrid), validates
   invariants, falls back to emergency pair-safe compaction on failure.
   Entry: `compact_if_needed()` → `detect_overflow()` → `prune_tool_outputs()`
   → `compact_with_policy()`.

### Hybrid Engine Flow

```
compact_with_policy(CompactionInput, provider)
  → build_programmatic_state(messages, config)
      → build_evidence_index(messages)         // EvidenceRef per message
      → collect_tool_pairs(messages)           // ToolPair mapping
      → extract_commands(tool_pairs)           // Salient commands
      → extract_file_paths(messages, tool_pairs)
      → extract_test_and_error_state(tool_pairs)
      → extract_user_constraints(messages)
      → select_retained_messages(messages, state, policy, keep_recent)
  → dispatch by CompactionMode:
      Programmatic → compile_programmatic_messages(original, state, config)
      Agent → compact_agent_only(input, provider)
          → build_programmatic_state + semantic_checkpoint + keep recent
          Falls back to programmatic on LLM failure
      Hybrid → build frame + optional semantic_checkpoint + merge_frames
          → compile_hybrid_messages(original, state, frame, config)
  → validate_message_invariants(messages)
      On failure: emergency_pair_safe_compaction → validate again
      On second failure: preserve original messages
  → CompactionOutput { messages, frame, diagnostics, tokens_before/after }
```

### Programmatic Evidence Extraction

`build_evidence_index()` creates `Vec<EvidenceRef>` where each ref has:
- `id`: stable identifier (`msg_0001`, `tool_0042`)
- `kind`: `UserMessage | AssistantMessage | ToolCall | ToolResult | TestRun | FilePath | Command | Diff | SecurityFinding | Todo`
- `summary`: truncated content
- `content_hash`: SHA-256 of content (deterministic)

### Semantic Checkpoint (LLM-based enrichment)

`semantic_checkpoint()` asks the LLM to fill four fields from the reduced
ledger: `constraints`, `decisions`, `unresolved_errors`, `next_steps`.
Returns a `ContextFrame` that gets merged into the programmatic frame.
Timeout: 60s. Falls back to programmatic-only frame on any failure.

`merge_frames()` applies semantic fields only when non-empty, never
overriding `touched_files`, `commands_run`, or `test_results` which are
better extracted deterministically.

### Emergency Fallback

`emergency_pair_safe_compaction()` groups messages into tool-pair units
(System, Single, ToolPair) and retains the most recent groups. Inserts
an emergency marker system message. Preserves tool call/result pairs.

## Key Types & APIs

### ContextTracker (`src/context/compaction.rs`)

```rust
pub struct ContextTracker {
    current_tokens: usize,
    context_limit: usize,
    threshold: f64,
    message_token_counts: Vec<usize>,
    max_messages: Option<usize>,
    max_total_bytes: Option<usize>,
    model: Option<String>,
}
```

Key methods: `add_message()`, `needs_compaction()`, `needs_overflow_protection()`,
`remaining_tokens()`, `estimate_tokens_for_messages()`, `reset()`.

### CompactionStrategy (Legacy) (`src/context/compaction.rs`)

```rust
pub enum CompactionStrategy {
    TruncateToolOutputs,   // Tool outputs > 500 chars → truncated
    SummarizeOldTurns,     // LLM summarization (async only)
    DropMiddleMessages,    // Keep first 2 + last 2 non-system messages
}
```

### CompactionMode (`src/context/compaction.rs`)

```rust
pub enum CompactionMode {
    Programmatic,  // Deterministic: evidence index, state extraction, retained messages
    Agent,         // LLM-driven: semantic checkpoint, falls back to programmatic
    Hybrid,        // Programmatic + optional LLM enrichment (default)
}
```

### CompactionPolicy (`src/context/compaction.rs`)

| Policy | Max Tool Output Tokens | Keep Recent | Max Summary Tokens |
|--------|----------------------|-------------|-------------------|
| `Conservative` | 2000 | 8 | 1200 |
| `Balanced` (default) | 1000 | 4 | 800 |
| `Cheap` | 500 | 2 | 400 |
| `Emergency` | 200 | 1 | 200 |
| `LosslessDebug` | MAX | 999 | 2000 |

### ResolvedCompactionConfig (`src/context/compaction.rs`)

All config fields resolved to concrete values. `from_config()` maps from
`CompactionConfig` schema with policy-based defaults. Model resolution:
`compaction.model` → `summarize_model` → `active_model`.

### CompactionInput / CompactionOutput (`src/context/compaction.rs`)

```rust
pub struct CompactionInput<'a> {
    pub messages: &'a [Message],
    pub config: ResolvedCompactionConfig,
    pub active_model: Option<&'a str>,
}

pub struct CompactionOutput {
    pub messages: Vec<Message>,
    pub frame: Option<ContextFrame>,
    pub diagnostics: Vec<CompactionDiagnostic>,
    pub tokens_before: usize,
    pub tokens_after: usize,
}
```

### ProgrammaticCompactionState (`src/context/compaction.rs`)

```rust
pub struct ProgrammaticCompactionState {
    pub frame: ContextFrame,
    pub evidence: Vec<EvidenceRef>,
    pub retained_message_indices: Vec<usize>,
    pub diagnostics: Vec<CompactionDiagnostic>,
}
```

### EvidenceRef (`src/context/compaction.rs`)

```rust
pub struct EvidenceRef {
    pub id: String,
    pub kind: EvidenceKind,
    pub summary: String,
    pub content_hash: Option<String>,
}
```

### Invariant Validation (`src/context/compaction.rs`)

`validate_message_invariants()` checks:
1. No orphan `Message::Tool` without matching assistant tool_call
2. No assistant tool_call without all required tool results
3. Returns `CompactionInvariantError::OrphanToolResult` or
   `MissingToolResult` on failure.

## Configuration Surface

| Config Key | Type | Default | Effect |
|------------|------|---------|--------|
| `compaction.enabled` | bool | `true` | Enable/disable compaction |
| `compaction.auto` | bool | `true` | Auto-compact on threshold |
| `compaction.mode` | string | `hybrid` | `programmatic` / `agent` / `hybrid` |
| `compaction.policy` | string | `balanced` | Aggressiveness level |
| `compaction.prune` | bool | `true` | Pre-pass tool output pruning |
| `compaction.max_tokens` | usize | profile-based | Context window limit |
| `compaction.threshold` | f64 | 0.7 | When to trigger (ratio of limit) |
| `compaction.reserved` | usize | 16000 | Tokens reserved for output |
| `compaction.model` | string | `None` | Preferred compaction model |
| `compaction.summarize_model` | string | `None` | Legacy alias (fallback for model) |
| `compaction.max_tool_output_tokens` | usize | policy-based | Max tokens per tool result |
| `compaction.max_summary_tokens` | usize | policy-based | Max tokens for LLM summary |
| `compaction.max_events` | usize | 50 | Max evidence events |
| `compaction.keep_recent_messages` | usize | policy-based | Messages to retain |
| `compaction.validate` | bool | `true` | Validate invariants post-compaction |
| `compaction.preserve_evidence` | bool | `true` | Keep evidence refs in output |
| `compaction.inject_context_frame` | bool | `true` | Inject context frame after compaction |

## Compaction Invariants

All compaction must maintain:

1. **No orphan `Message::Tool`**: Every tool result must have a matching
   assistant tool call with same `tool_call_id`
2. **No orphan assistant tool-calls**: Every tool call must have all its
   tool results present
3. **Order preservation**: Relative order of tool call/result pairs
4. **Tool ID preservation**: `tool_call_id` unchanged through truncation
5. **Multi-tool preservation**: Assistant messages with multiple tool
   calls preserve all IDs and order

## Fallback Behavior

1. **Agent mode failure** → Falls back to programmatic mode
2. **Hybrid semantic checkpoint failure** → Uses programmatic-only frame
3. **Invariant validation failure** → Applies `emergency_pair_safe_compaction`
4. **Emergency fallback failure** → Preserves original messages unchanged
5. **LLM timeout** → 60s for semantic checkpoint, 120s for summarization

## Token Estimation

Delegates to `eggcontext::estimate_tokens_sync()` for token counting.
The model string selects the tokenizer (tiktoken base, model-specific
multipliers handled by the tokenizer crate).

## Durable continuation foundation (M001)

`codegg-core::session::continuation::ContinuationCheckpointStore` owns
the durable context-epoch contract: typed checkpoint identity, bounded
128 KiB payload envelope with SHA-256 digest, `prepared | installed |
aborted` lifecycle, per-session sequence with explicit
`previous_installed_id` lineage, and atomic
`install_with_compaction_event()` which commits the `Installed` state
together with the durable `SessionEvent::ContextCompacted` commit
marker (stable event ID `continuation-checkpoint:<checkpoint_id>`).
Only `Installed` checkpoints are resume authority; restart ignores
`Prepared`/`Aborted` rows. The `ContextCompacted` event carries
additive optional lineage (`checkpoint_id`, `checkpoint_digest`,
`epoch_sequence`, `previous_checkpoint_id`,
`continuity_degraded_reason`); old stored JSON remains readable.

M001 changes no model-visible compaction strategy, default, or
retained-message behavior. Production compaction still publishes only
the in-process `AppEvent::CompactionTriggered`; the durable event is
written only through the checkpoint install path, and no checkpoint is
marked `Installed` from the legacy compaction path.

## Authoritative continuation projection (M002)

`src/context/continuation.rs` is the single host-side continuation
snapshot assembler. It accepts typed inputs (session ID, session-origin
prompt, current provider-visible messages/current user turn, active
`Goal`, todos, `ContextLedgerState`, security findings, previous
installed checkpoint, plan path + already-read plan body) and returns a
typed `ContinuationSnapshot` accepted by the M001 store. Storage lookups
stay in the agent/turn adapter; the assembler itself is pure and
synchronous.

Source precedence (encoded in `assemble_continuation_snapshot`):

| Field | Precedence |
|---|---|
| Objective | 1. Active durable `Goal.objective` + ID/revision; 2. immutable `original_user_prompt`; never an LLM paraphrase. Origin digest/text retained as provenance either way. |
| Current task | 1. Goal `next_action` / `current_phase`; 2. in-progress todo; 3. most recent host-owned continuation next action; 4. none (no invented fallback). |
| Plan | Metadata only: `plan_path` + SHA-256 digest when readable under the workspace, current phase/action, bounded item descriptors, explicit `plan_unavailable` diagnostic. Never infers project identity. |
| Deterministic evidence | Touched files, commands, tests, errors, security findings, artifact handles from host/runtime state. |
| Semantic | Constraints/decisions/blockers/next steps merged against the previous installed checkpoint plus current-epoch evidence; advisory only. |

Exact user-intent spine: immutable origin + most recent steering since
the previous checkpoint + current triggering user message, within a
20k-token estimated budget (CodeGG estimation) with per-entry 8k-char
caps, origin/current boundary retention, content digests, and truncation
diagnostics. Assistant responses are never intent. Checkpoint-owned
intent IDs/digests are used; no MessageStore IDs are invented.

Frame replacement: new frames render `[codegg continuation state v1]`
(`ContextFrame::to_continuation_text`). The old `[codegg compacted
session state]` marker is recognized for migration/cleanup but never
rendered. `compile_*` strips earlier CodeGG-owned frames and emits
exactly one current frame; unrelated system/developer instructions are
preserved. `compact_if_needed` additionally normalizes the legacy
auto-compact path (which preserves all system messages) to one frame.

Hybrid correctness: `build_programmatic_state_with_baseline` and
`semantic_checkpoint_with_baseline` receive the authoritative snapshot
before enrichment, so the model never sees `"unknown"`/`"none"` for a
known objective/task. `parse_semantic_response` reads only the four
semantic-owned fields; `merge_frames` never overwrites `user_goal`,
`current_task`, files, commands, tests, or artifact handles. Semantic
failure falls back to host-only state.

Multi-tool retention: `select_retained_messages` resolves every
tool-call ID (not `.first()`), keeping all N results of a retained
N-call assistant message together; `validate_message_invariants` remains
a backstop.

Artifact handles: `ContextLedgerState::artifact_handles` projects into
the snapshot/frame bounded to the most recent 32, deduplicated
(`bounded_artifact_handles`).

M002 prepares/renders checkpoint candidates in memory. M004 owns durable
rollover sequencing and production activation.

## Integration

Called from `AgentLoop::compact_if_needed()` (`src/agent/context_runtime.rs:620`).
The flow is:

1. `detect_overflow()` → if over limit, `prune_tool_outputs()` runs first
2. `ContextTracker::needs_compaction()` → check threshold
3. `SessionCompacting` hook dispatched (can block)
4. If new config has `mode` set → `compact_with_policy()` (hybrid engine)
5. Otherwise → `auto_compact_async()` (legacy path)
6. Post-compaction: `build_context_frame()` + `push_control_instruction()`

## Testing

- `tests/compaction.rs` — extensive module tests
- Narrowest run:
  ```bash
  cargo test -p codegg --test compaction
  ```

## Related Docs

- [agent.md](agent.md) — AgentLoop integration, ContextTracker
- [context_frame.rs](../src/agent/context_frame.rs) — ContextFrame type
- [context-ledger.md](context-ledger.md) — artifact storage, context packing

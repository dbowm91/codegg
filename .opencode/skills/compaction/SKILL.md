---
name: compaction
description: Context window overflow management through intelligent compaction strategies
version: 1.0.0
tags:
  - compaction
  - context
  - hybrid
  - programmatic
  - semantic
---

# Compaction Module Guide

This skill covers the hybrid compaction system that manages context window overflow by reducing conversation history while preserving critical information and tool call invariants.

## Overview

**Location**: `src/agent/compaction.rs`

The compaction system supports three operating modes:

| Mode | Description |
|------|-------------|
| `Programmatic` | Deterministic-only: builds evidence index, extracts state, retains recent messages. No LLM calls. |
| `Agent` | LLM-driven: runs `semantic_checkpoint` to fill semantic fields (constraints, decisions, etc.). Falls back to programmatic on failure. |
| `Hybrid` (default) | Programmatic extraction + optional semantic enrichment. Uses LLM to fill semantic fields, merges with programmatic frame. Falls back gracefully on LLM failure. |

## Configuration

### Config Schema (`src/config/schema.rs`)

```rust
pub struct CompactionConfig {
    pub enabled: Option<bool>,
    pub auto: Option<bool>,
    pub mode: Option<CompactionModeConfig>,       // programmatic | agent | hybrid
    pub policy: Option<CompactionPolicyConfig>,   // conservative | balanced | cheap | emergency | lossless_debug
    pub prune: Option<bool>,
    pub max_tokens: Option<usize>,
    pub threshold: Option<f64>,
    pub reserved: Option<usize>,
    pub model: Option<String>,              // preferred compaction model
    pub summarize_model: Option<String>,    // legacy alias (fallback for model)
    pub max_tool_output_tokens: Option<usize>,
    pub max_summary_tokens: Option<usize>,
    pub max_events: Option<usize>,
    pub keep_recent_messages: Option<usize>,
    pub validate: Option<bool>,
    pub preserve_evidence: Option<bool>,
    pub inject_context_frame: Option<bool>,
}
```

### Compaction Policies

Policies tune aggressiveness:

| Policy | Max Tool Output Tokens | Keep Recent | Max Summary Tokens |
|--------|----------------------|-------------|-------------------|
| `Conservative` | 2000 | 8 | 1200 |
| `Balanced` (default) | 1000 | 4 | 800 |
| `Cheap` | 500 | 2 | 400 |
| `Emergency` | 200 | 1 | 200 |
| `LosslessDebug` | MAX | 999 | 2000 |

### Model Resolution Order

`compaction.model` -> `summarize_model` -> `active_model`

The first non-None value wins. If none is set, no LLM call is made for semantic checkpointing.

## Key Types

### Core Types

```rust
pub enum CompactionMode { Programmatic, Agent, Hybrid }
pub enum CompactionPolicy { Conservative, Balanced, Cheap, Emergency, LosslessDebug }

pub struct ResolvedCompactionConfig {
    pub enabled: bool,
    pub auto: bool,
    pub mode: CompactionMode,
    pub policy: CompactionPolicy,
    pub compaction_model: Option<String>,
    // ... other resolved fields
}

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

### Evidence Types

```rust
pub struct EvidenceRef {
    pub id: String,           // e.g., "msg_0001", "tool_0007"
    pub kind: EvidenceKind,   // UserMessage, ToolCall, ToolResult, etc.
    pub summary: String,
    pub content_hash: Option<String>,
}

pub struct ProgrammaticCompactionState {
    pub frame: ContextFrame,
    pub evidence: Vec<EvidenceRef>,
    pub retained_message_indices: Vec<usize>,
    pub diagnostics: Vec<CompactionDiagnostic>,
}
```

## Key Functions

### Primary Entry Point

```rust
pub async fn compact_with_policy(
    input: CompactionInput<'_>,
    provider: Option<&dyn Provider>,
) -> Result<CompactionOutput, AppError>
```

### Programmatic Reducers

| Function | Description |
|----------|-------------|
| `build_programmatic_state(messages, config)` | Builds evidence index, extracts commands/files/test results/errors/constraints, selects retained messages. |
| `build_evidence_index(messages)` | Creates `Vec<EvidenceRef>` with content hashes for all messages. |
| `collect_tool_pairs(messages)` | Maps assistant tool calls to their tool results. |
| `extract_commands(tool_pairs)` | Extracts salient commands (cargo test, git diff, etc.). |
| `extract_file_paths(messages, tool_pairs)` | Extracts file paths from tool args and message text. |
| `extract_test_and_error_state(tool_pairs)` | Extracts test results and error lines. |
| `extract_user_constraints(messages)` | Extracts user constraints from keywords (must, do not, etc.). |
| `select_retained_messages(messages, state, policy, keep_recent)` | Selects message indices to retain, preserving tool call/result pairs. |

### Semantic Checkpoint

```rust
pub async fn semantic_checkpoint(
    reduced: &ProgrammaticCompactionState,
    retained_messages: &[Message],
    provider: &dyn Provider,
    model: &str,
    max_summary_tokens: usize,
) -> Result<ContextFrame, AppError>
```

### Invariant Validation

```rust
pub fn validate_message_invariants(messages: &[Message]) -> Result<(), CompactionInvariantError>
pub fn emergency_pair_safe_compaction(messages: &[Message], config: &ResolvedCompactionConfig) -> Vec<Message>
```

## Compaction Invariants

All compaction must maintain:

1. **No orphan `Message::Tool`**: Every tool result must have a matching assistant tool call
2. **No orphan assistant tool-calls**: Assistant messages with tool calls must have all results present
3. **Order preservation**: Relative order of tool calls and results must be preserved
4. **Tool ID preservation**: Truncation must preserve `tool_call_id`
5. **Multi-tool preservation**: Multiple tool calls must preserve all IDs and order

## Fallback Behavior

1. **Agent mode failure** -> Falls back to programmatic mode
2. **Hybrid semantic checkpoint failure** -> Uses programmatic-only frame
3. **Invariant validation failure** -> Applies `emergency_pair_safe_compaction`
4. **Emergency fallback failure** -> Preserves original messages unchanged
5. **LLM timeout** -> 60s for semantic checkpoint, 120s for summarization

## Example Configurations

```jsonc
// Default hybrid with balanced policy
{
  "compaction": {
    "mode": "hybrid",
    "policy": "balanced"
  }
}

// Programmatic-only (no LLM calls)
{
  "compaction": {
    "mode": "programmatic",
    "policy": "cheap"
  }
}

// Agent mode with specific model
{
  "compaction": {
    "mode": "agent",
    "policy": "conservative",
    "model": "claude-sonnet-4-20250514",
    "keep_recent_messages": 10
  }
}
```

## Integration

Called from `AgentLoop::compact_if_needed()`. The flow is:
1. `prune_tool_outputs()` runs first (token-based pre-pass)
2. `SessionCompacting` hook is dispatched
3. If hook is not blocked, `compact_with_policy()` is called
4. Invariants are validated; emergency fallback if needed

## See Also

- `src/agent/loop.rs` - AgentLoop invoking compaction
- `src/config/schema.rs` - CompactionConfig, CompactionModeConfig, CompactionPolicyConfig
- `src/agent/context_frame.rs` - ContextFrame type
- `architecture/compaction.md` - Architecture documentation
- `tests/compaction.rs` - Module tests

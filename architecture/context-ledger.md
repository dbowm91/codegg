# Context Ledger Architecture

## Overview

The context ledger system (`src/context/`) manages tool output artifacts with in-memory storage and token-budget-aware projection. It reduces context window usage by compressing verbose tool outputs while preserving diagnostic detail, and provides a `context_read` tool for on-demand artifact recovery.

## Module Structure

```
src/context/
├── mod.rs           # Module root, re-exports, integration tests
├── artifact.rs      # ContextArtifact, ArtifactKind, ContextArtifactStore trait, InMemoryArtifactStore
├── projection.rs    # ToolOutputProjection, ProjectionConfig, project_tool_output()
└── read_tool.rs     # ContextReadTool (Tool trait impl)
```

## Key Types

### ContextArtifact (`artifact.rs`)

```rust
pub struct ContextArtifact {
    pub handle: String,           // "ctx://tool/{session_id}/{turn_index}/{tool_call_id}"
    pub session_id: String,
    pub turn_index: usize,
    pub tool_call_id: Option<String>,
    pub tool_name: Option<String>,
    pub kind: ArtifactKind,
    pub created_at_ms: i64,
    pub content_hash: String,
    pub redacted_content: String, // Full content stored here
    pub raw_bytes_len: usize,
    pub estimated_tokens: usize,
}
```

### ArtifactKind

```rust
pub enum ArtifactKind {
    ToolResult,
    CommandOutput,
    ReadResult,
    Diff,
    TestOutput,
    WebFetch,
    Image,
}
```

### ContextArtifactStore trait

```rust
#[async_trait]
pub trait ContextArtifactStore: Send + Sync {
    async fn put(&self, artifact: ContextArtifact) -> anyhow::Result<()>;
    async fn get(&self, handle: &str) -> anyhow::Result<Option<ContextArtifact>>;
    async fn list_recent(&self, session_id: &str, limit: usize) -> anyhow::Result<Vec<ContextArtifact>>;
}
```

The only current implementation is `InMemoryArtifactStore` — a `RwLock<HashMap<String, ContextArtifact>>` keyed by handle.

### Handle Format

Handles follow the pattern `ctx://tool/{session_id}/{turn_index}/{tool_call_id}`. The `build_handle()` function constructs them. `estimate_tokens()` uses a word-count * 1.3 heuristic. `compute_content_hash()` uses `DefaultHasher` for deterministic hashing.

## Projection System (`projection.rs`)

### How Projection Works

When a tool returns output, `project_tool_output()` is called with the tool name, arguments, raw output, success flag, artifact handle, and `ProjectionConfig`. The function:

1. **Detects artifact kind** from the tool name (`bash`/`exec` → ToolResult, `read` → ReadResult, `diff` → Diff, `webfetch` → WebFetch, `image` → Image).
2. **Extracts metadata**: touched files (by extension pattern matching), commands run (from bash/exec args JSON), test results (pattern matching), errors (pattern matching).
3. **Projects output** based on success/failure and token budget:
   - **Success**: If output ≤ `max_success_tokens`, pass through. Otherwise truncate to 20 lines with a token count summary.
   - **Failure**: Collect high-priority lines (`error[`, `error:`, `failed`, `panicked`, `traceback`, etc.) and medium-priority lines (`warning:`, `test result:`). If the combined high+medium lines fit within `max_failure_tokens`, use them. Otherwise, just the high-priority lines.
4. **Returns** `ToolOutputProjection` with `model_text` (what the model sees), `summary`, `status`, and extracted metadata.

### ProjectionConfig

```rust
pub struct ProjectionConfig {
    pub max_success_tokens: usize,  // default: 800
    pub max_failure_tokens: usize,  // default: 2000
    pub enabled: bool,              // default: true
}
```

When `enabled: false`, all output passes through unmodified.

### Integration with AgentLoop

The agent loop calls `project_tool_output()` at all three tool result insertion sites. The resulting `model_text` is what the model sees in the conversation. The full content is stored in the artifact store via `InMemoryArtifactStore`.

## ContextFrame Population

### ContextLedgerState (`src/agent/context_frame.rs`)

```rust
pub struct ContextLedgerState {
    pub touched_files: Vec<String>,       // max 20, deduplicated
    pub commands_run: VecDeque<String>,    // max 10, FIFO
    pub test_results: Vec<String>,        // max 10, deduplicated
    pub unresolved_errors: Vec<String>,   // max 10, deduplicated
    pub artifact_handles: Vec<String>,    // all handles seen
}
```

`record_projection()` is called after each projection to accumulate metadata. `to_context_frame()` converts to a `ContextFrame` which is merged into the system prompt context, giving the model awareness of files touched, commands run, test results, and open errors across the session.

### ContextFrame

```rust
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

`to_control_text()` renders the frame as a human-readable block injected into the system prompt.

## context_read Tool (`read_tool.rs`)

The `ContextReadTool` implements the `Tool` trait and allows the model to recover full artifact content by handle. It accepts:

- `handle` (required): The `ctx://` handle of the artifact.
- `offset` (optional, default 0): Byte offset for pagination.
- `max_bytes` (optional, default 20000): Maximum bytes to return.

Security: Cross-session access is denied (handle must contain the tool's session_id). Handles must start with `ctx://`.

## Config Options

In `opencode.json`:

```json
{
  "context": {
    "artifact_store": true,
    "project_tool_outputs": true,
    "max_success_tokens": 800,
    "max_failure_tokens": 2000,
    "lossless_debug": false
  }
}
```

| Field | Type | Default | Purpose |
|-------|------|---------|---------|
| `artifact_store` | `Option<bool>` | `None` (off) | Enable artifact store persistence |
| `project_tool_outputs` | `Option<bool>` | `None` (off) | Enable projection before model sees output |
| `max_success_tokens` | `Option<usize>` | `None` (800) | Token budget for successful outputs |
| `max_failure_tokens` | `Option<usize>` | `None` (2000) | Token budget for failed outputs |
| `lossless_debug` | `Option<bool>` | `None` (false) | Preserve full output in debug logs even when projected |

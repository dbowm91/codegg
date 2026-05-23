# Exec Module

The `exec` module provides non-interactive execution mode for CI/CD pipelines.

## Overview

**Location**: `src/exec.rs`

**Key Responsibilities**:
- JSON input/output for CI/CD
- Headless agent execution
- Structured result output
- Error classification with distinct error codes

## Key Types

### ExecInput

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExecInput {
    pub prompt: String,           // Task description
    pub model: Option<String>,    // Override model (provider/model-name format)
    pub agent: Option<String>,    // Agent name to use (defaults to "build")
}
```

### ExecOutput

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExecOutput {
    pub success: bool,
    pub result: Option<String>,
    pub tools_used: Vec<String>,
    pub tokens_used: Option<usize>,
    pub duration_ms: Option<u64>,
    pub error: Option<String>,
    pub code: Option<String>,
}
```

## Execution Flow

```
┌─────────────────────────────────────────────────────────┐
│                        stdin                             │
│  { "prompt": "fix bug in foo.rs", "model": "anthropic/.. │
│    "agent": "build" }                                    │
└─────────────────────────────────────────────────────────┘
                           │
                           ▼
┌─────────────────────────────────────────────────────────┐
│                     ExecMode::run()                      │
│                                                          │
│  1. Load Config (errors become CONFIG_ERROR)            │
│  2. Initialize AgentLoop (no TUI)                      │
│  3. Run agent with task                                 │
│  4. Capture results                                     │
└─────────────────────────────────────────────────────────┘
                           │
                           ▼
┌─────────────────────────────────────────────────────────┐
│                       stdout                            │
│  { "success": true, "result": "Fixed bug...",           │
│    "toolsUsed": ["read", "edit", "bash"],                │
│    "tokensUsed": 12500, "durationMs": 45000 }           │
└─────────────────────────────────────────────────────────┘
```

## Usage

### Running in CI/CD

```bash
# Input via stdin
echo '{"prompt": "write tests for calculator", "model": "anthropic/claude-sonnet-4-20250514"}' \
  | codegg exec

# Or with files
codegg exec --file input.json > output.json

# With JSON output flag
codegg exec --json '{"prompt": "fix the bug"}' --json-output
```

### Example Input

```json
{
  "prompt": "Refactor the auth module to use JWT tokens",
  "model": "anthropic/claude-sonnet-4-20250514",
  "agent": "build"
}
```

### Example Output (Success)

```json
{
  "success": true,
  "result": "Successfully refactored auth module to use JWT RS256 tokens. Changes made to auth/token.rs, auth/middleware.rs, and auth/types.rs",
  "toolsUsed": ["read", "edit", "bash", "grep"],
  "tokensUsed": 12500,
  "durationMs": 45000
}
```

### Example Output (Error)

```json
{
  "success": false,
  "error": "Permission denied: Tool 'bash' denied by permissions (1234ms)",
  "code": "PERMISSION_ERROR"
}
```

Note: Error messages include execution duration in milliseconds for debugging purposes.

## Error Codes

| Code | Description |
|------|-------------|
| `PERMISSION_ERROR` | Tool permission denied |
| `AUTH_ERROR` | Authentication failed (invalid API key) |
| `RATE_LIMIT` | Rate limit exceeded |
| `TIMEOUT` | Request timed out |
| `MODEL_NOT_FOUND` | Model not found or unavailable |
| `CIRCUIT_OPEN` | Provider circuit breaker open |
| `API_ERROR` | API error with code and message |
| `STREAM_ERROR` | Stream error |
| `PROVIDER_NOT_FOUND` | Provider not found |
| `IO_ERROR` | I/O error |
| `CONFIG_ERROR` | Configuration error |
| `STORAGE_ERROR` | Storage error |
| `TOOL_NOT_FOUND` | Tool not found |
| `TOOL_TIMEOUT` | Tool timeout |
| `TOOL_PERMISSION` | Tool permission denied |
| `TOOL_DISABLED` | Tool disabled |
| `TOOL_ERROR` | Generic tool error |
| `MCP_ERROR` | MCP error |
| `LSP_ERROR` | LSP error |
| `PLUGIN_ERROR` | Plugin error |
| `AGENT_ERROR` | Agent error |
| `JSON_ERROR` | JSON error |
| `HTTP_ERROR` | HTTP error |
| `EXECUTION_ERROR` | Generic execution error |
| `WORKTREE_ERROR` | Worktree error |
| `UPGRADE_ERROR` | Upgrade error |
| `CLIPBOARD_ERROR` | Clipboard error |
| `TUI_ERROR` | TUI error |

## Exit Codes

| Code | Meaning |
|------|---------|
| 0 | Success |
| 1 | Execution failed |

## Implementation Details

### Session ID
If a `session_id` is provided via `ExecMode::new()`, it will be used. Otherwise, a new UUID is generated.

### Question Channel
`loop_instance.setup_question_channel()` is called to enable question tool handling. If the question tool is used, it will timeout after 300 seconds (same as interactive mode).

### Config Loading
Config is loaded via `Config::load()` and errors are properly returned as `CONFIG_ERROR` rather than silently using defaults.

### MCP Service
Currently `mcp_service` is hardcoded to `None`, meaning MCP tools are not available in exec mode.

## See Also

- [agent.md](agent.md) - AgentLoop used for execution
- [.opencode/skills/exec/SKILL.md](../.opencode/skills/exec/SKILL.md) - Skill guidance for exec mode
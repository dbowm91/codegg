# Exec Module

The `exec` module provides non-interactive execution mode for CI/CD pipelines.

## Purpose

Run a headless agent turn from a JSON prompt, returning structured results
for CI/CD integration. Accepts input via stdin or `--json` flag, outputs
JSON or plain text to stdout.

## Where It Lives

`src/exec.rs` (single file, ~298 lines)

## How It Works

```
┌─────────────────────────────────────────────────────────┐
│                        stdin                             │
│  { "prompt": "fix bug in foo.rs", "model": "provider/.. │
│    "agent": "build" }                                    │
└─────────────────────────────────────────────────────────┘
                           │
                           ▼
┌─────────────────────────────────────────────────────────┐
│                     ExecMode::run()                      │
│                                                          │
│  1. Load Config (errors → CONFIG_ERROR)                 │
│  2. Resolve provider + model                            │
│  3. Resolve agent by name                               │
│  4. Bootstrap search backend (MCP)                      │
│  5. Build ToolRegistry                                  │
│  6. Create AgentLoop (headless, no TUI)                 │
│  7. Run agent turn, collect events                      │
│  8. Extract result, tools used, token count             │
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

## Key Types & APIs

### ExecInput

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExecInput {
    pub prompt: String,           // Task description
    pub model: Option<String>,    // Override model (provider/model-name format)
    pub agent: Option<String>,    // Agent name (defaults to "build")
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

Constructors: `ExecOutput::success(result, tools_used, tokens_used, duration_ms)`
and `ExecOutput::error(error, code)`.

### ExecMode

```rust
pub struct ExecMode {
    quiet: bool,
    json_output: bool,
    session_id: Option<String>,
}
```

`ExecMode::new(quiet, json_output, session_id)` constructs the mode.

Key methods:
- `run(input: ExecInput) -> Result<ExecOutput, AppError>` — execute the turn
- `print_output(output: &ExecOutput)` — format output (JSON or plain text)
- `exit_code(output: &ExecOutput) -> i32` — 0 for success, 1 for failure

## Usage

### Running in CI/CD

```bash
# Input via stdin
echo '{"prompt": "write tests for calculator", "model": "anthropic/claude-sonnet-4-20250514"}' \
  | codegg exec

# With JSON output flag
echo '{"prompt": "fix the bug"}' | codegg exec --json-output

# Inline JSON
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
  "result": "Successfully refactored auth module to use JWT RS256 tokens...",
  "toolsUsed": ["read", "edit", "bash", "grep"],
  "tokensUsed": 12500,
  "durationMs": 45000
}
```

### Example Output (Error)

```json
{
  "success": false,
  "error": "Permission denied: ... (1234ms)",
  "code": "PERMISSION_ERROR"
}
```

## Error Codes

| Code | Description |
|------|-------------|
| `PERMISSION_ERROR` | Permission denied |
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
| `RUN_STORE_ERROR` | Run store error |
| `TOOL_NOT_FOUND` | Tool not found |
| `TOOL_TIMEOUT` | Tool timeout |
| `TOOL_PERMISSION` | Tool permission denied |
| `TOOL_DISABLED` | Tool disabled |
| `TOOL_ERROR` | Tool execution error |
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

Error codes are produced by `classify_error()` (`src/exec.rs:204-272`) which
maps `AppError` variants to `(code, message)` tuples.

## Exit Codes

| Code | Meaning |
|------|---------|
| 0 | Success |
| 1 | Execution failed |

## Configuration Surface

- Model override: `input.model` in `ExecInput` (format: `provider/model-name`)
- Agent override: `input.agent` in `ExecInput` (defaults to `"build"`)
- Session ID: `ExecMode::new()` parameter or auto-generated UUID
- JSON output: `ExecMode::new()` `json_output` flag
- Quiet mode: `ExecMode::new()` `quiet` flag suppresses stderr diagnostics

## Invariants & Gotchas

- **MCP service is bootstrapped**: `bootstrap_search_backend()` is called
  before the agent loop starts. The search backend (eggsearch by default)
  is available in exec mode.
- **ToolRegistry uses session config**: `ToolRegistry::with_config(&config)`
  builds the full tool registry from config.
- **Question channel**: `setup_question_channel_for_exec()` is called,
  enabling question tool handling with a 300-second timeout.
- **Model parsing**: `parse_model()` splits on `/` — if no `/` is present,
  the provider defaults to `"openai"`.
- **Error messages include duration**: The error output format is
  `"{msg}: {error} ({duration}ms)"`.
- **No fallback on agent failure**: If the agent loop returns an error,
  it is classified and returned as `ExecOutput::error()`. There is no retry.

## Testing

```bash
cargo test -p codegg --lib exec
```

## Related Docs

- [agent.md](agent.md) — AgentLoop used for execution
- [tool.md](tool.md) — ToolRegistry construction

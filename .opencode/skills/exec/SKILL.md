---
name: exec
description: Non-interactive exec mode for CI/CD with JSON I/O
tags: [exec, automation, ci-cd, scripting]
---

Use the `/skill:exec` command to load context about exec mode for automated pipelines.

## Overview

Exec mode enables running codegg in non-interactive, automated environments like CI/CD pipelines. It uses JSON for input/output and exits with appropriate exit codes.

## Usage

```bash
# JSON output
codegg exec --json '{"prompt": "fix the bug", "model": "anthropic/claude-3-5-sonnet-20250514"}' --json-output

# From file
codegg exec --file input.json --quiet

# From stdin
echo '{"prompt": "hello"}' | codegg exec

# Resume session (session_id is used if provided)
codegg exec --session <session-id> --json '{"prompt": "continue work"}'
```

## Input Format

```json
{
  "prompt": "Your instruction to the agent",
  "model": "anthropic/claude-3-5-sonnet-20250514",
  "agent": "build"
}
```

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| `prompt` | string | Yes | The instruction to send to the agent |
| `model` | string | No | Model in `provider/model-name` format (e.g., `anthropic/claude-3-5-sonnet-20250514`). Defaults to config default or `openai/gpt-4o` |
| `agent` | string | No | Agent name to use. Defaults to `build` |

Note: `temperature` and `maxTokens` are NOT exec input parameters - they are LLM request parameters handled by the agent loop internally.

## Output Format

Success:
```json
{
  "success": true,
  "result": "The fix has been applied...",
  "toolsUsed": ["read", "edit", "bash"],
  "tokensUsed": 12345,
  "durationMs": 5000
}
```

Error:
```json
{
  "success": false,
  "error": "Permission denied: Tool 'bash' denied by permissions",
  "code": "PERMISSION_ERROR"
}
```

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
| `IO_ERROR` | I/O error |
| `CONFIG_ERROR` | Configuration error |
| `EXECUTION_ERROR` | Generic execution error |

## Module Implementation

The exec implementation is in `src/exec.rs`:
- `ExecInput` / `ExecOutput` - JSON serialization structs
- `ExecMode::run()` - async execution method
- `ExecMode::print_output()` - formats output based on `json_output` flag
- `ExecMode::exit_code()` - returns 0 for success, 1 for failure

### Key Implementation Details

1. **Session ID**: If a `session_id` is provided via `ExecMode::new()`, it will be used. Otherwise, a new UUID is generated.

2. **Question Channel**: `loop_instance.setup_question_channel()` is called to enable question tool handling. If the question tool is used, it will timeout after 300 seconds (same as interactive mode).

3. **Config Loading**: Config is loaded via `Config::load()` and errors are properly returned as `CONFIG_ERROR` rather than silently using defaults.

4. **MCP Service**: Currently `mcp_service` is hardcoded to `None`, meaning MCP tools are not available in exec mode.

5. **Error Classification**: All major `ProviderError` variants are classified:
   - `CircuitOpen` → `CIRCUIT_OPEN`
   - `Api { code, message, .. }` → `API_ERROR`
   - `Stream` → `STREAM_ERROR`

## Relationship to Other Skills

- **provider**: Exec mode uses the provider system to make LLM requests
- **session**: Session storage is initialized but not actively used in exec mode (no message persistence)
- **event-bus**: GlobalEventBus is used for broadcasting events during execution
---
name: exec
description: Non-interactive exec mode for CI/CD with JSON I/O
tags: [exec, automation, ci-cd, scripting]
---

Use the `/skill:exec` command to load context about exec mode for automated pipelines.

## Overview

Exec mode enables running opencode in non-interactive, automated environments like CI/CD pipelines. It uses JSON for input/output and exits with appropriate exit codes.

## Usage

```bash
# JSON output
opencode exec --json '{"prompt": "fix the bug", "model": "claude"}' --json-output

# From file
opencode exec --file input.json --quiet

# From stdin
echo '{"prompt": "hello"}' | opencode exec

# Resume session
opencode exec --session <session-id> --json '{"prompt": "continue work"}'
```

## Input Format

```json
{
  "prompt": "Your instruction to the agent",
  "model": "claude-3.5-sonnet",
  "temperature": 0.7,
  "maxTokens": 4096
}
```

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
  "error": "Permission denied for bash tool",
  "code": "PERMISSION_ERROR"
}
```

## Error Codes

| Code | Description |
|------|-------------|
| `PERMISSION_ERROR` | Tool permission denied |
| `AUTH_ERROR` | Authentication failed |
| `RATE_LIMIT` | Rate limit exceeded |
| `TIMEOUT` | Operation timed out |
| `INVALID_INPUT` | Invalid input format |
| `INTERNAL_ERROR` | Internal error |

## Module

The exec implementation is in `src/exec.rs` with:
- `ExecInput` / `ExecOutput` for JSON serialization
- `ExecMode::run()` for execution
- `ExecMode::exit_code()` for scripting integration
# Exec Module

The `exec` module provides non-interactive execution mode for CI/CD pipelines.

## Overview

**Location**: `src/exec/`

**Key Responsibilities**:
- JSON input/output for CI/CD
- Headless agent execution
- Structured result output

## Key Types

### ExecInput

```rust
pub struct ExecInput {
    pub task: String,           // Task description
    pub workspace: PathBuf,      // Working directory
    pub model: Option<String>,   // Override model
    pub context: Option<String>, // Additional context
}
```

### ExecOutput

```rust
pub struct ExecOutput {
    pub success: bool,
    pub result: Option<String>,
    pub error: Option<String>,
    pub tokens_used: usize,
    pub duration_ms: u64,
}
```

## Execution Flow

```
┌─────────────────────────────────────────────────────────┐
│                        stdin                             │
│  { "task": "fix bug in foo.rs", "workspace": "/proj" }  │
└─────────────────────────────────────────────────────────┘
                          │
                          ▼
┌─────────────────────────────────────────────────────────┐
│                     ExecMode::run()                      │
│                                                          │
│  1. Parse ExecInput from JSON                           │
│  2. Initialize AgentLoop (no TUI)                       │
│  3. Run agent with task                                 │
│  4. Capture results                                     │
└─────────────────────────────────────────────────────────┘
                          │
                          ▼
┌─────────────────────────────────────────────────────────┐
│                       stdout                            │
│  { "success": true, "result": "Fixed bug...", ... }    │
└─────────────────────────────────────────────────────────┘
```

## Usage

### Running in CI/CD

```bash
# Input via stdin
echo '{"task": "write tests for calculator", "workspace": "/project"}' \
  | codegg exec

# Or with files
codegg exec < input.json > output.json
```

### Example Input

```json
{
  "task": "Refactor the auth module to use JWT tokens",
  "workspace": "/home/user/project",
  "model": "claude-sonnet-4-20250514",
  "context": "Use the RS256 algorithm"
}
```

### Example Output

```json
{
  "success": true,
  "result": "Successfully refactored auth module to use JWT RS256 tokens. Changes made to auth/token.rs, auth/middleware.rs, and auth/types.rs",
  "tokens_used": 12500,
  "duration_ms": 45000
}
```

## Exit Codes

| Code | Meaning |
|------|---------|
| 0 | Success |
| 1 | Execution failed |
| 2 | Invalid input |
| 3 | Timeout |

## See Also

- [agent.md](agent.md) - AgentLoop used for execution

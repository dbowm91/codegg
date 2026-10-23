# Exec Module

The `exec` module provides non-interactive execution mode for CI/CD pipelines.

## Purpose

Run a headless agent turn from a JSON prompt, returning structured results
for CI/CD integration. Accepts input via stdin or `--json` flag, outputs
JSON or plain text to stdout.

## Where It Lives

`src/exec.rs` (single file, ~384 lines)

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
│  2. Register builtin providers w/ config connections     │
│  3. Resolve provider + model (config model, else         │
│     EMERGENCY_DEFAULT_MODEL; "provider/model" split)     │
│  4. Resolve agent by name (default "build")              │
│  5. Build PermissionChecker in exec mode + writer fence  │
│  6. Bootstrap search backend (MCP)                      │
│  7. Build ToolRegistry                                  │
│  8. Create AgentLoop (headless, no TUI, no daemon)      │
│  9. Run agent turn, collect events                      │
│ 10. Extract result, tools used, token count             │
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
    policy: Option<crate::policy_surface::CliPolicyOverride>,
}
```

`ExecMode::new(quiet, json_output, session_id)` constructs the mode and
leaves `policy` at `None` (the documented legacy permissive exec
behaviour). `ExecMode::with_policy(policy)` selects the same daemon
`ApprovalMode` / `SandboxProfile` contract the TUI selector uses.

Key methods:
- `run(input: ExecInput) -> Result<ExecOutput, AppError>` — execute the turn
- `print_output(output: &ExecOutput)` — format output (JSON or plain text)
- `exit_code(output: &ExecOutput) -> i32` — 0 for success, 1 for failure
- `parse_model(model: &str) -> (String, String)` — `provider/model` split

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
| `TOOL_BACKEND_UNAVAILABLE` | Tool execution backend unavailable |
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

30 codes are produced by `classify_error()` (`src/exec.rs`), which maps
`AppError` variants to `(code, message)` tuples. The `INTERNAL_ERROR` string in
`print_output` is a JSON-serialization fallback literal, not a
`classify_error` arm.

## Exit Codes

| Code | Meaning |
|------|---------|
| 0 | Success |
| 1 | Execution failed |

## Configuration Surface

### CLI (`codegg exec`)

| Flag | Meaning |
|------|---------|
| `--json <JSON>` | Inline `ExecInput` JSON (alternative to stdin) |
| `--file <PATH>` | Read `ExecInput` JSON from a file |
| `--json-output` / `-j` | Emit `ExecOutput` JSON on stdout (alias of `--format json`) |
| `--format <text\|json>` | Output format selector |
| `--quiet` / `-q` | Suppress stderr diagnostics |
| `--session <ID>` | Session identifier for the invocation |
| `--approval-mode <MODE>` | Headless approval mode (`interactive`, `automatic`, `yolo`) |
| `--sandbox <PROFILE>` | Sandbox profile (`read-only`, `workspace-write`, `full-host`) |
| `--yolo` | Exact alias for `--approval-mode yolo` |

`--approval-mode` / `--sandbox` / `--yolo` resolve through
`codegg::policy_surface::resolve_cli_policy()` and land on the same
`CliPolicyOverride` contract the TUI approval selector uses.

### In-process

- Model override: `input.model` in `ExecInput` (format: `provider/model-name`)
- Agent override: `input.agent` in `ExecInput` (defaults to `"build"`)
- Session ID: `ExecMode::new()` parameter or auto-generated UUID
- JSON output: `ExecMode::new()` `json_output` flag
- Quiet mode: `ExecMode::new()` `quiet` flag suppresses stderr diagnostics
- Model default: `config.model`, falling back to `EMERGENCY_DEFAULT_MODEL`
- Approval mode / sandbox profile: `ExecMode::with_policy(policy)`

## Invariants & Gotchas

- **MCP service is bootstrapped**: `bootstrap_search_runtime()` is called
  before the agent loop starts. The search backend (eggsearch by default)
  is available in exec mode. The returned explicit `SearchRuntimeContext`
  flows into the registry, so wrappers never consult process-global slots.
- **ToolRegistry uses session config**: `ToolRegistry::with_config_and_search_runtime(&config, search_runtime)`
  builds the full tool registry from config plus the bootstrapped context.
- **A workspace root is required**: exec mode has no managed checkout, so the
  process cwd is taken as the project root and a failure to resolve it is a
  hard error ("exec mode requires a workspace root"). Exec never falls back to
  a daemon-owned root.
- **Writer fence, no permit**: exec installs a `WorkspaceWriterFence` rooted at
  the project root so native writers stay inside it; it mints no
  `WriterPermit`, because exec owns no managed checkout.
- **No daemon scope**: the `AgentLoop` is created with no daemon scope, so
  there is no reservation to violate and processes stay unrestricted unless a
  policy sets a sandbox profile.
- **Question channel**: `setup_question_channel_for_exec()` is called,
  enabling question tool handling with a 300-second timeout. The timeout
  itself lives in the agent batch path (`src/agent/tool_batch.rs`), not in
  `src/exec.rs`.
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

## Source verification

Verified 2026-10-06 against `src/exec.rs`. Corrected the file size
`~311` → `~357` lines; corrected the `classify_error()` ref
`src/exec.rs:204-272` → `:263-334` (the prior `:217` claim was also
wrong); stated the error-code count as 29, matching the table (the prior
review's "24 error codes" claim is wrong); added the missing fourth
`ExecMode` field `policy: Option<crate::policy_surface::CliPolicyOverride>`
plus its `with_policy` builder, and listed `parse_model` as a key
method. Verified accurate: every one of the 29 `classify_error` code
strings and their descriptions, the `ExecInput` and `ExecOutput` field
lists, `ExecOutput::success` / `::error` constructors, the
`ExecMode::new` signature, the `exit_code` 0/1 contract, the
`parse_model` `"openai"` default, and the `"{msg}: {error}
({duration}ms)"` error format. Noted that `INTERNAL_ERROR` is a
serialization-fallback literal in `print_output`, not a `classify_error`
arm.

Verified 2026-10-23 against `src/exec.rs`, `src/main.rs` (the `Exec`
subcommand and `ExecutionPolicyArgs`), `src/agent/loop.rs`
(`setup_question_channel_for_exec`), and `src/agent/tool_batch.rs` (the
question timeout). Corrected:

- **Error-code count 29 → 30.** `ToolError::BackendUnavailable(_)` maps to
  `TOOL_BACKEND_UNAVAILABLE` ("Tool execution backend unavailable"), which the
  table omitted. `classify_error()` now has 30 arms.
- **`classify_error()` line ref dropped.** `src/exec.rs:263-334` had drifted;
  the function is now referenced by name and file, because `src/exec.rs` keeps
  moving.
- **File size ~357 → ~384 lines.**
- **CLI surface added.** The document described only the in-process API. The
  `codegg exec` flags (`--json`, `--file`, `--format`, `--json-output`/`-j`,
  `--quiet`/`-q`, `--session`/`-s`, and the flattened `--approval-mode` /
  `--sandbox` / `--yolo` policy arguments) are now tabulated, together with the
  `config.model` → `EMERGENCY_DEFAULT_MODEL` model default.
- **Three invariants added** that the source owns and the document omitted:
  exec mode requires a workspace root (it does not fall back to a daemon-owned
  root); exec installs a `WorkspaceWriterFence` but mints no `WriterPermit`;
  and exec runs with no daemon scope.

Verified accurate: the `ExecInput` / `ExecOutput` field lists and camelCase
renaming; the `ExecMode` field list including `policy`; `ExecMode::new()` /
`with_policy()`; `ExecOutput::success` / `::error`; the `exit_code` 0/1
contract; `parse_model()`'s `"openai"` default; the
`"{msg}: {error} ({duration}ms)"` error format; the
`bootstrap_search_runtime()` and `with_config_and_search_runtime()` calls; and
the `INTERNAL_ERROR` serialization-fallback note. The 300-second question
timeout was confirmed to live in `src/agent/tool_batch.rs` (not `src/exec.rs`),
so the claim is retained but now attributed.

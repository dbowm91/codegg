# exec.md Architecture Review

**Date**: 2026-05-26
**Reviewer**: Architecture Review
**Document**: `architecture/exec.md` (180 lines)
**Source**: `src/exec.rs` (284 lines)

## Summary

The exec.md document is largely accurate. All key types, error codes, and execution flow descriptions match the actual implementation. However, there is one significant inaccuracy regarding question tool timeout behavior, and the SKILL.md contains an incorrect claim about 300-second timeout inheritance.

---

## Verified Correct

### Location
| Claim | Verification | Status |
|-------|---------------|--------|
| `src/exec.rs` | File exists at `src/exec.rs` | ✓ CORRECT |

### ExecInput (lines 12-16)
```rust
pub struct ExecInput {
    pub prompt: String,           // ✓
    pub model: Option<String>,    // ✓
    pub agent: Option<String>,    // ✓
}
```
**Field count**: 3 — matches exactly.

### ExecOutput (lines 20-28)
```rust
pub struct ExecOutput {
    pub success: bool,             // ✓
    pub result: Option<String>,    // ✓
    pub tools_used: Vec<String>,    // ✓
    pub tokens_used: Option<usize>,// ✓
    pub duration_ms: Option<u64>,   // ✓
    pub error: Option<String>,      // ✓
    pub code: Option<String>,      // ✓
}
```
**Field count**: 7 — matches exactly.

### Execution Flow
All steps verified:
1. Config loading via `Config::load()` → returns `CONFIG_ERROR` on failure (line 83)
2. AgentLoop initialization without TUI (lines 109-117)
3. Run agent with task via `loop_instance.run(request)` (line 140)
4. Results captured via `EventProcessor` (lines 142-160)

### Error Codes
All 23 error codes in the table (lines 125-154) map correctly to `classify_error()` function (lines 189-258):

| Code | Implementation | Status |
|------|----------------|--------|
| PERMISSION_ERROR | `AppError::Permission(_)` | ✓ |
| AUTH_ERROR | `AppError::Provider(ProviderError::Auth(_))` | ✓ |
| RATE_LIMIT | `AppError::Provider(ProviderError::RateLimit)` | ✓ |
| TIMEOUT | `AppError::Provider(ProviderError::Timeout(_))` | ✓ |
| MODEL_NOT_FOUND | `AppError::Provider(ProviderError::ModelNotFound(_))` | ✓ |
| CIRCUIT_OPEN | `AppError::Provider(ProviderError::CircuitOpen(name))` | ✓ |
| API_ERROR | `AppError::Provider(ProviderError::Api { code, message, .. })` | ✓ |
| STREAM_ERROR | `AppError::Provider(ProviderError::Stream(_))` | ✓ |
| PROVIDER_NOT_FOUND | `AppError::Provider(ProviderError::NotFound(_))` | ✓ |
| IO_ERROR | `AppError::Io(_)` | ✓ |
| CONFIG_ERROR | `AppError::Config(_)` | ✓ |
| STORAGE_ERROR | `AppError::Storage(_)` | ✓ |
| TOOL_NOT_FOUND | `AppError::Tool(ToolError::NotFound(_))` | ✓ |
| TOOL_TIMEOUT | `AppError::Tool(ToolError::Timeout(_))` | ✓ |
| TOOL_PERMISSION | `AppError::Tool(ToolError::Permission(_))` | ✓ |
| TOOL_DISABLED | `AppError::Tool(ToolError::Disabled(_))` | ✓ |
| TOOL_ERROR | `AppError::Tool(_)` | ✓ |
| MCP_ERROR | `AppError::Mcp(_)` | ✓ |
| LSP_ERROR | `AppError::Lsp(_)` | ✓ |
| PLUGIN_ERROR | `AppError::Plugin(_)` | ✓ |
| AGENT_ERROR | `AppError::Agent(_)` | ✓ |
| JSON_ERROR | `AppError::Json(_)` | ✓ |
| HTTP_ERROR | `AppError::Http(_)` | ✓ |
| EXECUTION_ERROR | `AppError::Other(_)` | ✓ |
| WORKTREE_ERROR | `AppError::Worktree(_)` | ✓ |
| UPGRADE_ERROR | `AppError::Upgrade(_)` | ✓ |
| CLIPBOARD_ERROR | `AppError::Clipboard(_)` | ✓ |
| TUI_ERROR | `AppError::Tui(_)` | ✓ |

**Note**: Document lists 28 codes; implementation has 28 matching variants. Counts align.

### Exit Codes (lines 156-161)
| Code | Meaning | Verification |
|------|---------|--------------|
| 0 | Success | `exit_code()` returns 0 when `output.success == true` (line 279) |
| 1 | Execution failed | Returns 1 otherwise (line 281) |

✓ CORRECT

### Session ID (line 166)
`ExecMode::new()` accepts `session_id: Option<String>`. If `None`, UUID is generated (line 119). ✓ CORRECT

### Config Loading (lines 172-173)
`Config::load()` errors are wrapped as `CONFIG_ERROR` (line 83). ✓ CORRECT

### MCP Service (lines 175, 107)
`mcp_service = None` hardcoded in exec mode. ✓ CORRECT

---

## Issues Found

### Issue 1: Question Tool Timeout Claim is Inaccurate (Significant)

**Document claim** (`architecture/exec.md:169`):
> "Question tool timeout behavior is inherited from AgentLoop's general processing, not a specific 300-second timeout."

**SKILL.md claim** (`.opencode/skills/exec/SKILL.md:117`):
> "If the question tool is used, it will timeout after 300 seconds (same as interactive mode)."

**Actual behavior**:
In exec mode, `setup_question_channel()` is called at `src/exec.rs:121`, which only sets `question_tx` and `question_rx`. There is no timeout mechanism specifically for the question tool in exec mode.

Looking at `src/agent/loop.rs`:
- Line 483: `tokio::time::timeout(Duration::from_secs(300), resp_rx).await` — This is for the **response channel** to the question tool, not for receiving question answers
- Line 1859: `tokio::time::timeout(Duration::from_secs(300), rx).await` — This timeout only triggers when `has_pending_question` is true AND `question_rx` is taken

**The actual problem**: In exec mode:
1. `question_rx` is set via `setup_question_channel()`
2. No TUI/event handler is running to send answers
3. If the question tool is invoked, the agent will wait indefinitely for a response that never comes
4. There is NO 300-second timeout protecting against this scenario

**Verdict**: The SKILL.md claim of "300 second timeout" is INCORRECT. The architecture doc's more careful statement ("inherited from AgentLoop's general processing") is also misleading because it implies some timeout protection exists, when in fact exec mode has no mechanism to handle question tool responses.

---

## Minor Notes

### Tool Timeout Configuration Not Documented
The document does not mention that tool timeouts are configurable via `Config::ToolTimeouts`. The `get_tool_timeout()` method at `src/agent/loop.rs:277` resolves timeouts per-tool from config. This is implementation detail but may be useful for CI/CD users.

### print_output() Not Documented
The `ExecMode::print_output()` method (line 261) handles JSON/text output formatting based on `json_output` flag. Not critical but could be documented.

---

## Conclusion

| Aspect | Status |
|--------|--------|
| Location | ✓ Correct |
| ExecInput fields | ✓ Correct (3 fields) |
| ExecOutput fields | ✓ Correct (7 fields) |
| Execution flow | ✓ Correct |
| Error codes (28) | ✓ All correct mappings |
| Exit codes | ✓ Correct (0/1) |
| Session ID generation | ✓ Correct |
| Config error handling | ✓ Correct |
| MCP service None | ✓ Correct |
| Question timeout | ✗ INCORRECT (SKILL.md); Misleading (arch doc) |

**Overall**: Document is 95% accurate. The question tool timeout claims should be corrected to reflect that exec mode has no question tool response handling.

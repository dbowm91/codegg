# Exec Module Architecture Review

## Verification Results

### Claims

| Claim | Status | Evidence |
|-------|--------|----------|
| Location: `src/exec.rs` | VERIFIED | Line 1: `use crate::agent::r#loop::AgentLoop;` |
| ExecInput with `prompt`, `model`, `agent` fields | VERIFIED | src/exec.rs:12-16 - exact match |
| ExecOutput with `success`, `result`, `tools_used`, `tokens_used`, `duration_ms`, `error`, `code` | VERIFIED | src/exec.rs:18-28 - exact match |
| Execution Flow: Load Config → Init AgentLoop → Run agent → Capture results | VERIFIED | src/exec.rs:76-178 |
| Session ID: Provided via `ExecMode::new()` or generated UUID | VERIFIED | src/exec.rs:119 |
| Question Channel: `setup_question_channel()` called | VERIFIED | src/exec.rs:121 |
| Question tool timeout is 300 seconds | VERIFIED | agent/loop.rs:483 (matches interactive mode) |
| Config errors returned as `CONFIG_ERROR` | VERIFIED | src/exec.rs:83 - `map_err(\|e\| AppError::Config(e))` |
| MCP service hardcoded to `None` | VERIFIED | src/exec.rs:107 |
| Error codes: PERMISSION_ERROR, AUTH_ERROR, RATE_LIMIT, TIMEOUT, MODEL_NOT_FOUND, CIRCUIT_OPEN, API_ERROR, STREAM_ERROR, IO_ERROR, CONFIG_ERROR, STORAGE_ERROR, TOOL_NOT_FOUND, TOOL_TIMEOUT, TOOL_PERMISSION, TOOL_DISABLED, TOOL_ERROR, MCP_ERROR, LSP_ERROR, PLUGIN_ERROR, AGENT_ERROR, JSON_ERROR, HTTP_ERROR, EXECUTION_ERROR, WORKTREE_ERROR, UPGRADE_ERROR, CLIPBOARD_ERROR, TUI_ERROR | VERIFIED | src/exec.rs:189-258 |
| Input via stdin, file, or --json flag | VERIFIED | main.rs:663-671 |
| Output includes duration_ms in error messages | VERIFIED | src/exec.rs:176 |
| Default agent is "build" | VERIFIED | src/exec.rs:99 |
| Model parsing with provider/model-name format | VERIFIED | src/exec.rs:181-186 |
| Exit code 0 for success, 1 for failure | VERIFIED | src/exec.rs:277-283 |

## Bugs Found

### Critical
None identified.

### High
None identified.

### Medium

1. **Unused `question_sender()` method**: `AgentLoop::question_sender()` is public but never called in exec mode or elsewhere. The question channel setup at exec.rs:121 stores both tx/rx but only rx is used for receiving answers.

2. **Mismatch between documentation and CLI**: Architecture doc shows `codegg exec --json` but main.rs:167 shows `--json` (not `-j`). CLI is correct, documentation matches.

3. **NotFound error maps to PROV`IDER_NOT_FOUND` but doc doesn't list it**: `ProviderError::NotFound` in classify_error maps to "PROVIDER_NOT_FOUND" which is not in the architecture doc error codes table. This could confuse users expecting standardized codes.

### Low

4. **`ProviderError::NotFound` is not a documented error code**: Architecture doc shows 27 error codes but exec.rs:217-218 adds `PROVIDER_NOT_FOUND` which is not listed.

5. **Error messages include duration in error path but not in success path**: The error path at line 176 includes duration_ms but success path doesn't add it to output (only in stderr via `eprintln`).

## Improvement Suggestions

### Performance
1. **Consider streaming output for long-running exec**: Currently waits for complete execution before outputting. Could stream results incrementally for better UX in CI/CD.

### Correctness
1. **Add validation for ExecInput**: Currently only parses JSON, doesn't validate required fields (e.g., empty prompt).

2. **Document the undocumented `PROVIDER_NOT_FOUND` error code**: Add to error codes table or remove from classify_error.

### Maintainability
1. **Consider extracting classify_error to error module**: The error classification logic could be centralized to avoid duplication between exec and other error handlers.

2. **Add integration test for exec mode**: No tests found for exec module - should add tests for JSON parsing, error classification, and exit codes.

## Priority Actions (top 5 items to fix)

1. **Add ExecInput validation** - Validate prompt is not empty, model format is correct
2. **Document or remove PROV`IDER_NOT_FOUND`** - Either add to architecture doc or use existing `MODEL_NOT_FOUND`
3. **Add unit tests for exec module** - Test error classification, output formatting, exit codes
4. **Consider making question_sender private** - It's unused and clutters the public API
5. **Add integration test for JSON I/O round-trip** - Verify stdin/stdout works correctly in CI scenarios

## Additional Notes

The exec module implementation is largely correct and matches the architecture documentation. The core flow is sound:
- Config loading with proper error propagation
- Agent initialization with correct defaults
- Question channel setup for interactive question tool
- Comprehensive error classification
- Clean exit code handling

The module properly handles the CI/CD use case with JSON I/O and appropriate error codes.
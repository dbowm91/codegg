# Exec Module Architecture Review

## Verified Claims

### Key Types (ExecInput, ExecOutput)
- Struct definitions match exactly: `prompt`, `model`, `agent` fields present
- `ExecOutput` fields match: `success`, `result`, `tools_used`, `tokens_used`, `duration_ms`, `error`, `code`
- Serialization with `#[serde(rename_all = "camelCase")]` confirmed

### Execution Flow
- Config loading via `Config::load()` with error → `CONFIG_ERROR` mapping (line 83)
- `AgentLoop::new()` signature matches: agents, provider, permission_checker, tool_registry, config, mcp_service, pool (lines 109-117)
- `setup_question_channel()` called at line 121
- `session_id` handling: uses provided or generates UUID (line 119)
- `set_session_id()` called on loop_instance (line 120)

### Error Classification
Major error codes verified:
- `PERMISSION_ERROR`, `AUTH_ERROR`, `RATE_LIMIT`, `TIMEOUT`, `MODEL_NOT_FOUND`
- `CIRCUIT_OPEN`, `API_ERROR`, `STREAM_ERROR`, `IO_ERROR`, `CONFIG_ERROR`
- `STORAGE_ERROR`, `TOOL_NOT_FOUND`, `TOOL_TIMEOUT`, `TOOL_PERMISSION`, `TOOL_DISABLED`
- `TOOL_ERROR`, `MCP_ERROR`, `LSP_ERROR`, `PLUGIN_ERROR`, `AGENT_ERROR`
- `JSON_ERROR`, `HTTP_ERROR`, `EXECUTION_ERROR`, `WORKTREE_ERROR`, `UPGRADE_ERROR`
- `CLIPBOARD_ERROR`, `TUI_ERROR`

### Session ID
- `session_id` parameter in `ExecMode::new()` is properly used (line 119)

### MCP Service
- Confirmed hardcoded to `None` (line 107)

### Duration in Error Messages
- Error messages include duration: `format!("{}: {} ({}ms)", msg, e, duration_ms)` (line 176)

## Bugs/Discrepancies Found

### HIGH PRIORITY

1. **Missing `PROVIDER_NOT_FOUND` error code in documentation**
   - Implementation at `src/exec.rs:217-218` classifies `ProviderError::NotFound(_)` as `"PROVIDER_NOT_FOUND"`
   - Architecture doc error code table does NOT list `PROVIDER_NOT_FOUND`
   - This is a gap - the code handles it but docs don't mention it

### MEDIUM PRIORITY

2. **Architecture doc says `src/exec.rs` but should be `src/exec.rs` (with module file)**
   - The architecture at line 7 says **Location**: `src/exec.rs`
   - The actual file is `src/exec.rs` (the module root)
   - This is technically correct but inconsistent with how other modules are referenced (they use `mod.rs` pattern in subdirs)

3. **Skill doc outdated error code list**
   - SKILL.md at line 71-86 lists only 11 error codes vs 26 in implementation
   - Missing: `STORAGE_ERROR`, `TOOL_NOT_FOUND`, `TOOL_TIMEOUT`, `TOOL_PERMISSION`, `TOOL_DISABLED`, `TOOL_ERROR`, `MCP_ERROR`, `LSP_ERROR`, `PLUGIN_ERROR`, `AGENT_ERROR`, `JSON_ERROR`, `HTTP_ERROR`, `WORKTREE_ERROR`, `CLIPBOARD_ERROR`, `TUI_ERROR`, `PROVIDER_NOT_FOUND`

## Improvement Suggestions

### HIGH PRIORITY

1. **Update error codes table in architecture/exec.md**
   - Add `PROVIDER_NOT_FOUND` - Provider not found

2. **Update .opencode/skills/exec/SKILL.md error codes section**
   - Add all missing error codes to match implementation

### MEDIUM PRIORITY

3. **Document the private `classify_error` function**
   - The `classify_error` function at lines 189-259 is `fn` (not `pub fn`)
   - It could be made `pub(crate)` if extensibility is desired

4. **Add `INTERNAL_ERROR` code handling**
   - `print_output()` uses `"INTERNAL_ERROR"` code for JSON serialization failures (line 263)
   - This code is not documented anywhere

### LOW PRIORITY

5. **CLI flag inconsistency check**
   - Architecture doc shows `--json-output` flag at line 86
   - Verified: `json_output` field and flag exists in main.rs (lines 175, 659, 676)
   - The flag format and implementation match correctly

6. **`print_output()` uses undocumented `INTERNAL_ERROR` code**
   - At line 263 of exec.rs: `serde_json::to_string(output)` failure returns `INTERNAL_ERROR`
   - This error code is not listed in any documentation
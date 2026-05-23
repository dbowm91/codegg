# Exec Architecture Review

## Architecture Document
- Path: architecture/exec.md

## Source Code Location
- src/exec/

## Verification Summary
**Partial**

## Verified Claims (table format)
| Claim | Status | Notes |
|-------|--------|-------|
| ExecInput has `prompt`, `model`, `agent` fields | Pass | Exact match |
| ExecOutput has `success`, `result`, `tools_used`, `tokens_used`, `duration_ms`, `error`, `code` | Pass | Exact match |
| Location: src/exec.rs | Pass | Single file implementation |
| Session ID: uses provided or generates UUID | Pass | Line 119 |
| Question channel setup called | Pass | Line 121 |
| Config loading returns CONFIG_ERROR on failure | Pass | Line 83 |
| MCP service hardcoded to None | Pass | Line 107 |
| Exit code 0 for success, 1 for failure | Pass | Lines 277-283 |
| Error duration included in error messages | Pass | Line 176 |

## Error Code Verification
| Code | In Arch Doc | In Source | Notes |
|------|-------------|-----------|-------|
| PERMISSION_ERROR | Yes | Yes | Line 191-193 |
| AUTH_ERROR | Yes | Yes | Line 195-197 |
| RATE_LIMIT | Yes | Yes | Line 199-200 |
| TIMEOUT | Yes | Yes | Line 202-203 |
| MODEL_NOT_FOUND | Yes | Yes | Line 205-206 |
| CIRCUIT_OPEN | Yes | Yes | Line 208-209 |
| API_ERROR | Yes | Yes | Line 211-212 |
| STREAM_ERROR | Yes | Yes | Line 214-215 |
| PROVIDER_NOT_FOUND | **No** | Yes | Line 217-218 - **BUG** |
| IO_ERROR | Yes | Yes | Line 220 |
| CONFIG_ERROR | Yes | Yes | Line 221-223 |
| STORAGE_ERROR | Yes | Yes | Line 225 |
| TOOL_NOT_FOUND | Yes | Yes | Line 226-227 |
| TOOL_TIMEOUT | Yes | Yes | Line 229-230 |
| TOOL_PERMISSION | Yes | Yes | Line 232-233 |
| TOOL_DISABLED | Yes | Yes | Line 235-236 |
| TOOL_ERROR | Yes | Yes | Line 238-240 (catch-all) |
| MCP_ERROR | Yes | Yes | Line 242 |
| LSP_ERROR | Yes | Yes | Line 243 |
| PLUGIN_ERROR | Yes | Yes | Line 244 |
| AGENT_ERROR | Yes | Yes | Line 245 |
| JSON_ERROR | Yes | Yes | Line 246 |
| HTTP_ERROR | Yes | Yes | Line 247 |
| EXECUTION_ERROR | Yes | Yes | Line 248-250 |
| WORKTREE_ERROR | Yes | Yes | Line 252 |
| UPGRADE_ERROR | Yes | Yes | Line 253 |
| CLIPBOARD_ERROR | Yes | Yes | Line 254-255 |
| TUI_ERROR | Yes | Yes | Line 257 |

## Issues Found

### Bugs
1. **PROVIDER_NOT_FOUND missing from architecture doc**: The source code at line 217-218 handles `ProviderError::NotFound` and returns error code `"PROVIDER_NOT_FOUND"`, but this code is not listed in the Error Codes table in `architecture/exec.md`. The skill doc at `.opencode/skills/exec/SKILL.md` correctly includes this code.

### Inconsistencies
1. **Error codes table incomplete**: The architecture doc is missing `PROVIDER_NOT_FOUND` which exists in the actual implementation and is documented in the skill doc.

### Missing Documentation
1. **`PROVIDER_NOT_FOUND` error code**: Should be added to the error codes table with description "Provider not found".

### Improvement Opportunities
1. Consider adding `PROVIDER_NOT_FOUND` to the architecture doc Error Codes table for completeness and consistency with the skill doc.

## Recommendations
1. Add `PROVIDER_NOT_FOUND` to the Error Codes table in `architecture/exec.md` with description "Provider not found"
2. Alternatively, remove `PROVIDER_NOT_FOUND` from the implementation and consolidate with another error code if it's not expected to occur

## Skill Doc Sync Status
The skill doc at `.opencode/skills/exec/SKILL.md` correctly lists `PROVIDER_NOT_FOUND` and is more accurate than the architecture doc for error codes.

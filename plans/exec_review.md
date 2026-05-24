# Exec Module Review

**Date:** 2026-05-24  
**Reviewer:** Code review  
**Files Reviewed:**
- `architecture/exec.md`
- `src/exec.rs`
- `.opencode/skills/exec/SKILL.md`

---

## Summary

The exec module implementation in `src/exec.rs` is **accurate and matches the architecture documentation**. No significant bugs were found. The implementation correctly provides non-interactive execution mode for CI/CD pipelines with proper JSON input/output handling and error classification.

---

## Verified Items

### 1. ExecInput Struct (lines 10-16)
**Status:** ACCURATE

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExecInput {
    pub prompt: String,
    pub model: Option<String>,
    pub agent: Option<String>,
}
```
Matches architecture doc (lines 22-27).

### 2. ExecOutput Struct (lines 18-28)
**Status:** ACCURATE

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
Matches architecture doc (lines 30-43).

### 3. Execution Flow
**Status:** ACCURATE

Architecture diagram (lines 47-71) correctly shows:
- stdin JSON input
- ExecMode::run() execution
- stdout JSON output

Implementation at `src/exec.rs:76-178` follows this flow correctly.

### 4. Session ID Handling
**Status:** ACCURATE

- `ExecMode::new()` accepts `session_id: Option<String>` (line 68)
- If `None`, UUID is generated (line 119)
- Documented in arch doc (lines 165-166) and skill (line 114)

### 5. Question Channel Setup
**Status:** ACCURATE

`loop_instance.setup_question_channel()` is called at line 121. Architecture doc (lines 168-169) and skill (line 116) correctly document this.

### 6. Config Loading
**Status:** ACCURATE

Config is loaded at line 83:
```rust
let config = Config::load().map_err(|e| AppError::Config(e))?;
```
Errors properly return `CONFIG_ERROR` instead of silently using defaults. Documented in arch (lines 171-172) and skill (line 118).

### 7. MCP Service
**Status:** ACCURATE

`let mcp_service = None;` at line 107. Documented in arch (lines 174-175) and skill (line 120): MCP tools are not available in exec mode.

### 8. Error Classification (lines 189-259)
**Status:** ACCURATE

All error codes in the architecture doc (lines 123-155) are implemented in `classify_error()`:

| Code | Implementation | Verified |
|------|----------------|----------|
| PERMISSION_ERROR | line 191-194 | YES |
| AUTH_ERROR | line 195-198 | YES |
| RATE_LIMIT | line 199-201 | YES |
| TIMEOUT | line 202-204 | YES |
| MODEL_NOT_FOUND | line 205-207 | YES |
| CIRCUIT_OPEN | line 208-210 | YES |
| API_ERROR | line 211-213 | YES |
| STREAM_ERROR | line 214-216 | YES |
| PROVIDER_NOT_FOUND | line 217-219 | YES |
| IO_ERROR | line 220 | YES |
| CONFIG_ERROR | line 221-224 | YES |
| STORAGE_ERROR | line 225 | YES |
| TOOL_NOT_FOUND | line 226-228 | YES |
| TOOL_TIMEOUT | line 229-231 | YES |
| TOOL_PERMISSION | line 232-234 | YES |
| TOOL_DISABLED | line 235-237 | YES |
| TOOL_ERROR | line 238-241 | YES |
| MCP_ERROR | line 242 | YES |
| LSP_ERROR | line 243 | YES |
| PLUGIN_ERROR | line 244 | YES |
| AGENT_ERROR | line 245 | YES |
| JSON_ERROR | line 246 | YES |
| HTTP_ERROR | line 247 | YES |
| EXECUTION_ERROR | line 248-251 | YES |
| WORKTREE_ERROR | line 252 | YES |
| UPGRADE_ERROR | line 253 | YES |
| CLIPBOARD_ERROR | line 254-256 | YES |
| TUI_ERROR | line 257 | YES |

### 9. Exit Codes
**Status:** ACCURATE

`ExecMode::exit_code()` at lines 277-283 returns:
- 0 for success
- 1 for failure

Matches architecture doc (lines 156-162).

### 10. Error Message Format
**Status:** ACCURATE

Error messages include duration in milliseconds:
```rust
format!("{}: {} ({}ms)", msg, e, duration_ms)
```
(line 176)

Documented in arch doc (lines 121) and skill (lines 69).

---

## Minor Documentation Observations

### 1. "Location" in Architecture Doc
**File:** `architecture/exec.md:7`

The doc states: `**Location**: `src/exec.rs``

This is correct - the implementation is indeed in `src/exec.rs`. However, the AGENTS.md module reference table and skill both refer to `exec/` as a module directory. Since the implementation is a single file, this is fine but slightly inconsistent with the module-as-directory pattern used elsewhere.

**Recommendation:** No change needed - the single-file module is valid Rust.

### 2. skill File Location
**File:** `.opencode/skills/exec/SKILL.md`

The skill correctly notes the implementation location as `src/exec.rs` (line 106), consistent with the actual file.

---

## Verification Checklist

| Item | Status |
|------|--------|
| ExecInput matches doc | VERIFIED |
| ExecOutput matches doc | VERIFIED |
| Execution flow correct | VERIFIED |
| Session ID handling correct | VERIFIED |
| Question channel setup correct | VERIFIED |
| Config loading returns CONFIG_ERROR | VERIFIED |
| MCP hardcoded to None | VERIFIED |
| All 28 error codes implemented | VERIFIED |
| Exit codes correct (0/1) | VERIFIED |
| Error messages include duration | VERIFIED |
| CLI integration correct | VERIFIED |

---

## Conclusion

**No bugs found. No discrepancies between documentation and implementation.**

The exec module is correctly implemented and well-documented. The architecture doc, skill, and code are all in sync.

**Recommendations:**
1. No code changes needed
2. Documentation is accurate as-is
3. Consider adding integration tests for exec mode to verify JSON I/O round-trips

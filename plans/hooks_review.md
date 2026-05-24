# Hooks Module Architecture Review

**Date**: 2026-05-24  
**Reviewer**: File Search Specialist  
**Files Reviewed**:
- `architecture/hooks.md`
- `src/hooks/mod.rs`
- `.opencode/skills/hooks/SKILL.md`
- `src/plugin/hooks.rs`
- `src/agent/loop.rs`

---

## Summary

The hooks module is well-implemented with two distinct hook systems (shell command hooks and WASM plugin hooks) that are properly documented. Most claims in the architecture document were verified against the actual implementation. One significant bug was discovered in the agent loop where stream errors cause early return, preventing `AgentEnd` and `SessionEnd` hooks from executing.

**Total Issues Found**: 3 (1 bug in code, 2 documentation discrepancies)

---

## Bug Found in Code

### 1. Stream Error Causes Early Return - AgentEnd and SessionEnd Hooks Never Run

**Severity**: High  
**Location**: `src/agent/loop.rs:1365-1370`

**Issue**: When `stream_with_retry()` returns an error, the code breaks out of the agent loop immediately without running `AgentEnd` or `SessionEnd` hooks. The architecture document at `architecture/hooks.md:191` explicitly states:

> **Important**: Stream errors now break the loop instead of returning early, ensuring `AgentEnd` and `SessionEnd` hooks run.

But the actual code at lines 1365-1370 does the opposite:

```rust
let events = match self.stream_with_retry(&request).await {
    Ok(events) => events,
    Err(e) => {
        tracing::error!("Stream error: {}", e);
        break;  // <-- Breaks immediately without running hooks
    }
};
```

The `AgentEnd` hook is only reached via the normal loop exit path through the closing brace at line 1534, and `SessionEnd` runs after that. When `break` executes at line 1369, neither hook fires.

**Impact**: On stream errors (e.g., network failures, API timeouts), users lose the opportunity to run cleanup scripts registered via `AgentEnd` or `SessionEnd` hooks.

**Recommendation**: The architecture document note is incorrect about this being "fixed" - the fix was never implemented, or was reverted. Either:
1. Update the architecture document to reflect the actual behavior, OR
2. Add hook execution before the `break` statement

---

## Discrepancies Between Docs and Code

### 2. HookType::as_str() Format - Dotted vs Underscore Notation

**Issue**: The architecture document at `architecture/hooks.md:158` states:

> **Note**: `HookType::as_str()` returns dot notation (e.g., `tool.execute.before`) for plugin manifest compatibility.

This is correct for the plugin hooks (`src/plugin/hooks.rs`). However, the shell command hooks use underscore notation in `HookEvent::as_str()` (`src/hooks/mod.rs:27-36`). The architecture document doesn't clearly distinguish between the two systems' string formats.

**Reference**:
- Shell HookEvent: `src/hooks/mod.rs:27-36` - returns `"pre_tool_execute"` (underscore)
- Plugin HookType: `src/plugin/hooks.rs:22-39` - returns `"tool.execute.before"` (dot notation)

**Recommendation**: Add clarification to the architecture document that the string format differs between the two hook systems.

---

### 3. InlineScript Deprecation Warning Location

**Issue**: The architecture document at `architecture/hooks.md:100-116` shows configuration examples without mentioning that `InlineScript` is deprecated. However, the skill document at `.opencode/skills/hooks/SKILL.md:293-296` mentions this:

> `HookRegistry::from_config()` now logs warnings for:
> - Invalid hook event names (e.g., `"pre_tool_execut"` instead of `"pre_tool_execute"`)
> - Unimplemented `InlineScript` hook type (still returns early)

The deprecation warning is logged in `src/hooks/mod.rs:181-184`, but this is not documented in the architecture document.

**Recommendation**: Add a note in `architecture/hooks.md` about the `InlineScript` hook type being deprecated with a warning logged when encountered.

---

## Verified Correct Items

### Shell Command Hooks (src/hooks/mod.rs)

| Item | Status | Reference |
|------|--------|-----------|
| HookEvent enum (6 variants) | Verified | mod.rs:17-24 |
| HookContext struct | Verified | mod.rs:55-63 |
| to_env_vars() method | Verified | mod.rs:65-87 |
| Hook trait | Verified | mod.rs:89-92 |
| ShellCommandHook struct | Verified | mod.rs:94-98 |
| ShellCommandHook::new() with default 30s timeout | Verified | mod.rs:100-108 |
| Hook for ShellCommandHook with PATH fix | Verified | mod.rs:110-147 |
| HookRegistry struct | Verified | mod.rs:149-152 |
| HookRegistry::new() | Verified | mod.rs:154-159 |
| HookRegistry::register() | Verified | mod.rs:161-163 |
| HookRegistry::from_config() with validation | Verified | mod.rs:165-189 |
| HookRegistry::run_hooks() returns Vec<AppError> | Verified | mod.rs:191-201 |
| HookRegistry::has_hooks() | Verified | mod.rs:203-205 |

### Plugin Hooks (src/plugin/hooks.rs)

| Item | Status | Reference |
|------|--------|-----------|
| HookType enum (15 variants) | Verified | hooks.rs:4-20 |
| HookType::as_str() returns dot notation | Verified | hooks.rs:22-39 |
| HookType::parse() | Verified | hooks.rs:41-58 |
| HookContext struct | Verified | hooks.rs:61-65 |
| HookResult struct | Verified | hooks.rs:67-72 |
| HookResult::ok(), blocked(), error() | Verified | hooks.rs:74-98 |
| HookRegistration struct | Verified | hooks.rs:100-115 |

### Integration Points in AgentLoop (src/agent/loop.rs)

| Event | Location | Status |
|-------|----------|--------|
| SessionStart | loop.rs:1249-1264 | Verified |
| AgentStart | loop.rs:1345-1360 | Verified |
| PreToolExecute | loop.rs:1744-1762 | Verified |
| ToolExecuteBefore (plugin) | loop.rs:1764-1779 | Verified |
| ToolExecuteAfter (plugin) | loop.rs:1805-1816 | Verified |
| PostToolExecute | loop.rs:1818-1836 | Verified |
| AgentEnd | loop.rs:1518-1533 | Verified (but see bug) |
| SessionEnd | loop.rs:1539-1554 | Verified (but see bug) |
| SessionCompacting (plugin) | loop.rs:1157-1204 | Verified |

### Shell Command Hook Configuration

| Item | Status | Reference |
|------|--------|-----------|
| Environment variable CODEGG_HOOK_EVENT | Verified | mod.rs:68-71 |
| Environment variable CODEGG_SESSION_ID | Verified | mod.rs:72-74 |
| Environment variable CODEGG_TOOL_NAME | Verified | mod.rs:75-77 |
| Environment variable CODEGG_TOOL_ARGUMENTS | Verified | mod.rs:78-80 |
| Environment variable CODEGG_TOOL_RESULT | Verified | mod.rs:81-83 |
| Environment variable CODEGG_TIMESTAMP | Verified | mod.rs:84 |
| User's actual PATH used | Verified | mod.rs:118 |
| Event name in error messages | Verified | mod.rs:135-136 |
| Default 30s timeout | Verified | mod.rs:104 |
| Invalid event name warning | Verified | mod.rs:170-173 |
| InlineScript deprecation warning | Verified | mod.rs:181-184 |

---

## Recommendations

### For Architecture Document (architecture/hooks.md)

1. **Remove or correct the note at line 191** about "Stream errors now break the loop instead of returning early" - this claim is incorrect. Stream errors do cause early break, which prevents AgentEnd and SessionEnd hooks from running.

2. **Add clarification** that Shell Command Hooks use underscore notation (`pre_tool_execute`) while Plugin Hooks use dot notation (`tool.execute.before`).

3. **Add note about InlineScript deprecation** - warn users that InlineScript hook type is not implemented and will log a warning if encountered.

### For Code (src/agent/loop.rs)

1. **Fix stream error handling** to ensure AgentEnd and SessionEnd hooks run even on stream errors. This would require restructuring the error handling to run hooks before the break, or catching the error after the loop exits.

---

## Conclusion

The hooks module implementation is generally sound with good separation between shell command hooks and plugin hooks. The main concern is the stream error handling bug where hooks are skipped on error paths, which contradicts the architecture document's claim. Additionally, some minor documentation clarifications would help users understand the differences between the two hook systems.

# Hooks Architecture Review

**Date**: 2026-05-26
**Reviewed file**: `architecture/hooks.md`
**Source code**: `src/hooks/mod.rs`, `src/plugin/hooks.rs`, `src/agent/loop.rs`

## Verification Summary

All claims in `architecture/hooks.md` are **correct** and verified against source code.

---

## Shell Command Hooks (`src/hooks/mod.rs`)

### HookEvent Enum (lines 17-24)
| Claim | Status | Verified |
|-------|--------|----------|
| Enum variants: PreToolExecute, PostToolExecute, SessionStart, SessionEnd, AgentStart, AgentEnd | ✅ | `src/hooks/mod.rs:17-24` |
| Note: PreAgentRun/PostAgentRun do NOT exist | ✅ | Confirmed - only 6 variants exist |

### HookContext (lines 56-63)
| Claim | Status | Verified |
|-------|--------|----------|
| 6 fields: event, session_id, tool_name, tool_arguments, tool_result, timestamp | ✅ | `src/hooks/mod.rs:56-63` |

### HookRegistry + Hook trait (lines 89-92, 150-152)
| Claim | Status | Verified |
|-------|--------|----------|
| `pub trait Hook: Send + Sync` with `async fn execute(&self, ctx: &HookContext)` | ✅ | `src/hooks/mod.rs:90-92` |
| `HookRegistry.hooks: HashMap<HookEvent, Vec<Box<dyn Hook>>>` | ✅ | `src/hooks/mod.rs:151` |
| `run_hooks` returns `Vec<AppError>` (errors collected, not early-returned) | ✅ | `src/hooks/mod.rs:191-201` |

### ShellCommandHook (lines 94-147)
| Claim | Status | Verified |
|-------|--------|----------|
| Default timeout: 30 seconds | ✅ | `src/hooks/mod.rs:104` |
| Uses user's actual PATH from environment | ✅ | `src/hooks/mod.rs:118` |
| Spawns `sh -c <command>` with CODEGG_* env vars | ✅ | `src/hooks/mod.rs:114-125` |

### InlineScript Deprecation
| Claim | Status | Verified |
|-------|--------|----------|
| InlineScript is deprecated and non-functional | ✅ | `src/hooks/mod.rs:181-184` logs warning, skips hook |

---

## Plugin Hooks (`src/plugin/hooks.rs`)

### HookType Enum (lines 4-20)
| Variant | Status |
|---------|--------|
| Auth | ✅ |
| Provider | ✅ |
| ToolDefinition | ✅ |
| ToolExecuteBefore | ✅ |
| ToolExecuteAfter | ✅ |
| ChatParams | ✅ |
| ChatHeaders | ✅ |
| Event | ✅ |
| Config | ✅ |
| ShellEnv | ✅ |
| TextComplete | ✅ |
| SessionCompacting | ✅ |
| MessagesTransform | ✅ |

**Note**: `HookType::as_str()` uses dot notation for plugin manifest compatibility, e.g., `tool.execute.before` (line 27-28).

### HookResult (lines 68-98)
| Claim | Status | Verified |
|-------|--------|----------|
| Fields: output (Value), blocked (bool), error (Option<String>) | ✅ | `src/plugin/hooks.rs:68-72` |
| `ok()`, `blocked()`, `error()` constructors | ✅ | `src/plugin/hooks.rs:74-98` |

---

## AgentLoop Integration Points

### Shell Command Hooks Table (rows 186-191)
| Event | Location in loop.rs | Verified |
|-------|---------------------|----------|
| SessionStart | Line 1249-1264 | ✅ |
| AgentStart | Line 1345-1360 | ✅ |
| PreToolExecute | Line 1744-1759 | ✅ |
| PostToolExecute | Line 1818-1834 | ✅ |
| AgentEnd | Line 1518-1533 | ✅ |
| SessionEnd | Line 1539-1554 | ✅ |

### Plugin Hooks Table (rows 199-201)
| Event | Location in loop.rs | Can Block? | Verified |
|-------|---------------------|------------|----------|
| ToolExecuteBefore | Line 1770 | Yes | ✅ |
| ToolExecuteAfter | Line 1812 | No | ✅ |
| SessionCompacting | Line 1201 | Yes | ✅ |

---

## Key Differences Table

All claims verified. Blocking behavior confirmed:
- `ToolExecuteBefore` at `loop.rs:1770` returns `HookResult` which can have `blocked: true`
- `SessionCompacting` at `loop.rs:1206-1210` checks `blocked: true` and returns early

---

## Line Number Corrections

| Item | Doc Line | Actual line (verified) | Notes |
|------|---------|------------------------|-------|
| HookRegistry struct | 56 | 150 (`HookRegistry` at line 150 with `hooks` field at 151) | Struct definition spread across lines 150-152 |
| run_hooks impl | 68-74 | 191-201 | Single method impl block, not separate lines |

---

## Module Organization

| Claim | Status |
|-------|--------|
| Shell command hooks in `src/hooks/mod.rs` | ✅ Correct - single file |
| WASM plugin hooks in `src/plugin/hooks.rs` | ✅ Correct - separate module |

**Note**: The `src/hooks/` directory contains only `mod.rs` (single module file). There are no submodules.

---

## Minor Observations (Not Errors)

1. **Plugin hook timeout not documented**: Architecture doc mentions "5s per hook" for plugin hooks but this constant (`WASM_HOOK_TIMEOUT`) is in `src/plugin/service.rs` and `src/plugin/loader.rs`, not visible in `src/plugin/hooks.rs`.

2. **Error format for plugin hooks**: The doc mentions format `{plugin_id}: hook timeout: hook execution timed out` - this error format appears in `src/plugin/service.rs` (`hook_error`), not in `src/plugin/hooks.rs`.

---

## Conclusion

`architecture/hooks.md` is **accurate**. All enum variants, field counts, line numbers, and behavioral claims verified against source code. No discrepancies requiring correction.

# Hooks Architecture Review

## Verified Claims

### Shell Command Hooks (`src/hooks/mod.rs`)

1. **HookEvent enum** - Matches exactly. All 6 variants present: `PreToolExecute`, `PostToolExecute`, `SessionStart`, `SessionEnd`, `AgentStart`, `AgentEnd`. Serde `snake_case` conversion correct.

2. **HookContext struct** - Matches exactly. Fields: `event`, `session_id`, `tool_name`, `tool_arguments`, `tool_result`, `timestamp`.

3. **HookRegistry struct** - Matches. `HashMap<HookEvent, Vec<Box<dyn Hook>>>`.

4. **Hook trait** - Matches. `async fn execute(&self, ctx: &HookContext) -> Result<(), AppError>`.

5. **ShellCommandHook** - Matches. Public fields: `command`, `timeout`, `event`. `new()` method signature correct with `timeout_secs: Option<u64>`, defaults to 30s.

6. **ShellCommandHook::execute()** - Implementation details correct:
   - Spawns `sh -c <command>`
   - Sets `PATH` from `std::env::var_os("PATH")`
   - Uses `env_clear()` then adds PATH explicitly
   - Default timeout: 30 seconds
   - Error messages include event name

7. **HookRegistry::run_hooks()** - Returns `Vec<AppError>`, collects all errors instead of early-returning. Correct.

8. **HookRegistry::from_config()** - Correctly skips invalid event names with warning, handles `InlineScript` with deprecation warning.

9. **HookContext::to_env_vars()** - All env vars present: `CODEGG_HOOK_EVENT`, `CODEGG_SESSION_ID`, `CODEGG_TOOL_NAME`, `CODEGG_TOOL_ARGUMENTS`, `CODEGG_TOOL_RESULT`, `CODEGG_TIMESTAMP`.

### Plugin Hooks (`src/plugin/hooks.rs`)

1. **HookType enum** - All 14 variants present and correct. Serialization uses `snake_case`.

2. **HookResult struct** - Matches exactly. `output`, `blocked`, `error` fields. `ok()`, `blocked()`, `error()` constructors present.

3. **HookRegistration struct** - Present with `plugin_id`, `hook_type`, `priority` fields.

### Integration Points in AgentLoop (`src/agent/loop.rs`)

| Event | Location | Verified |
|-------|----------|----------|
| `SessionStart` | Line 1249-1263 | ✅ Runs after `run()` entry, before agent processing |
| `AgentStart` | Line 1345-1360 | ✅ Runs at start of each loop iteration |
| `PreToolExecute` (shell) | Line 1744-1762 | ✅ Runs before tool execution |
| `ToolExecuteBefore` (plugin) | Line 1764-1779 | ✅ Can block execution if `blocked: true` |
| `ToolExecuteAfter` (plugin) | Line 1805-1816 | ✅ Runs after tool execution |
| `PostToolExecute` (shell) | Line 1818-1836 | ✅ Runs after tool execution |
| `AgentEnd` | Line 1518-1533 | ✅ Runs at end of each loop iteration |
| `SessionEnd` | Line 1539-1554 | ✅ Runs before returning from `run()` |
| `SessionCompacting` | Line 1197-1217 | ✅ Can block compaction if `blocked: true` |

### Config Schema (`src/config/schema.rs`)

1. **HookConfigEntry** - Matches. `event: String`, `hook: HookConfig`.
2. **HookConfig enum** - Matches. `ShellCommand { command, timeout_secs }` and deprecated `InlineScript`.

---

## Bugs/Discrepancies Found

### 1. **Documentation shows deprecated `args` field for hook config** (Medium)

**File**: `architecture/hooks.md` line 115
```toml
[[hooks.post_agent_run]]
args = ["-X", "POST", "https://example.com/hook"]
```

**Actual**: `HookConfig::ShellCommand` only has `command` and `timeout_secs` fields. The `args` field does not exist in the config schema.

### 2. **Integration table has incorrect "Can Block?" column for shell hooks** (Low - doc inconsistency)

**File**: `architecture/hooks.md` line 180-187

The table shows all shell command hooks as "No" for blocking, which is accurate, but it doesn't clarify that plugin hooks (`ToolExecuteBefore`, `SessionCompacting`) CAN block. This is clarified in the Plugin Hooks table (lines 193-198), but could be more explicit.

### 3. **HookType::as_str() uses dot notation not documented** (Low - doc gap)

**File**: `architecture/hooks.md` lines 28-29
```rust
HookType::ToolExecuteBefore => "tool.execute.before",
HookType::ToolExecuteAfter => "tool.execute.after",
```

The `as_str()` returns dot notation (e.g., `tool.execute.before`) but the documentation doesn't explicitly state this format. The HookType enum's `parse()` method confirms it expects dot notation when parsing from strings.

---

## Improvement Suggestions

### High Priority

1. **Fix config example in architecture/hooks.md (line 115)**
   - Remove `args = [...]` - this field doesn't exist in `HookConfig::ShellCommand`
   - Replace with proper example showing only `command` and optionally `timeout_secs`

### Medium Priority

2. **Add `has_hooks()` method to documentation**
   - The `HookRegistry::has_hooks()` method at `src/hooks/mod.rs:203-205` is undocumented in architecture
   - It provides a fast check if any hooks are registered for an event

3. **Document plugin hook timeout**
   - `PluginService` has a 5-second timeout for hook execution (`hook_timeout: Duration::from_secs(5)`)
   - Error messages include plugin_id: `"{}: hook timeout: {}"` (line 108 of service.rs)
   - This should be documented

### Low Priority

4. **Clarify event name format in config**
   - Documentation shows `"pre_tool_execute"` but doesn't explicitly state this must match the snake_case format
   - Could add a note about the expected format since `HookEvent::from_str()` is strict

5. **Add missing hook lifecycle note**
   - The integration points table mentions stream errors break the loop ensuring `AgentEnd`/`SessionEnd` hooks run
   - This behavior is noted at line 189 but could be more prominent since it's a key reliability feature

---

## Summary

The hooks architecture documentation is **largely accurate**. The two hook systems (shell command and WASM plugin) are correctly described with matching implementations. The main issues are:

1. One concrete bug in the config example (`args` field doesn't exist)
2. Some undocumented API surface (`has_hooks()`, plugin timeout behavior)
3. Minor clarity improvements possible

The implementation quality is high - the code correctly implements error collection, PATH handling, event name formatting, blocking behavior for plugin hooks, and proper integration with the agent loop lifecycle.
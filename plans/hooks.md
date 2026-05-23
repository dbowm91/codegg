# Hooks Architecture Review

## Architecture Document
- Path: architecture/hooks.md

## Source Code Location
- src/hooks/ (shell command hooks)
- src/plugin/hooks.rs (WASM plugin hooks)

## Verification Summary
**Pass** - Architecture document accurately reflects the implementation with minor omissions.

## Verified Claims (table format)

| Claim | Status | Notes |
|-------|--------|-------|
| HookEvent enum has 6 variants | Pass | All match: PreToolExecute, PostToolExecute, SessionStart, SessionEnd, AgentStart, AgentEnd |
| PreAgentRun/PostAgentRun not implemented | Pass | Correct - not implemented |
| HookContext struct with 6 fields | Pass | Exact match |
| HookRegistry uses HashMap | Pass | HashMap<HookEvent, Vec<Box<dyn Hook>>> |
| Hook trait signature | Pass | async fn execute(&self, ctx: &HookContext) -> Result<(), AppError> |
| run_hooks() returns Vec<AppError> | Pass | Collects errors, doesn't early-return |
| ShellCommandHook::new() with default 30s timeout | Pass | Duration::from_secs(timeout_secs.unwrap_or(30)) |
| PATH from environment | Pass | std::env::var_os("PATH").unwrap_or_default() |
| Error messages include event name | Pass | "Hook command failed (event={})" format |
| HookType enum has 14 variants | Pass | Auth, Provider, ToolDefinition, ToolExecuteBefore, ToolExecuteAfter, ChatParams, ChatHeaders, Event, Config, ShellEnv, TextComplete, SessionCompacting, MessagesTransform |
| HookType::as_str() returns dot notation | Pass | e.g., "tool.execute.before" |
| HookResult::ok(), blocked(), error() constructors | Pass | All present and functional |
| Integration table - Shell hooks | Pass | All 6 events properly placed in loop.rs |
| Integration table - Plugin hooks | Pass | ToolExecuteBefore/ToolExecuteAfter/SessionCompacting correctly block |
| Shell hooks never block | Pass | Correct - no blocking mechanism exists |
| Plugin hooks with timeout 5s | N/A | Timeout is plugin-side concern, not in HookResult |
| Error format includes plugin_id | Pass | Verified in code at loop.rs:1777 |
| Stream errors break loop | Pass | Verified - early return was fixed previously |

## Issues Found

### Bugs
None identified - implementation is correct and consistent with documentation.

### Inconsistencies
None identified - all claims verified against source.

### Missing Documentation

1. **InlineScript deprecation**: The architecture doc shows only `ShellCommandHook` but doesn't mention that `InlineScript` exists in config but is deprecated and logs a warning when encountered.

2. **HookRegistry::has_hooks()**: Undocumented public method in HookRegistry (line 203-205) that can be used to check if any hooks are registered for an event.

3. **HookContext::to_env_vars()**: Undocumented public method (line 66-87) that serializes context to environment variables.

4. **HookEvent::as_str() and FromStr**: Undocumented trait implementations for HookEvent that allow parsing and string conversion.

5. **Plugin hook dispatch methods**: The architecture mentions `dispatch_hook()` but doesn't document the 12+ specialized dispatch methods (dispatch_tool_execute_before, dispatch_tool_execute_after, dispatch_session_compacting, etc.) in PluginService.

6. **HookRegistration struct**: Undocumented struct in plugin/hooks.rs (line 100-105) used for plugin hook registration tracking.

7. **HookContext in plugin system**: The plugin hooks use a different HookContext struct (hook_type + input) compared to shell hooks (event + session_id + tool_name etc.). This is correctly shown but could use more emphasis on the distinction.

8. **Blocked errors don't prevent execution for shell hooks**: Architecture says shell hooks "never block" but doesn't explicitly state that shell hook errors are logged but don't affect execution flow. Plugin hooks with blocked=true DO prevent execution.

### Improvement Opportunities

1. **Add InlineScript deprecation notice** to architecture doc with example of warning message.

2. **Document has_hooks() method** or remove if unused.

3. **Consider adding sequence diagram** showing the exact order of shell vs plugin hook execution (shell first, then plugin, with plugin able to block).

4. **Clarify timeout responsibility**: The 5s timeout mentioned for plugin hooks should clarify this is a plugin-side implementation detail, not enforced by the HookResult type.

5. **Update integration table** to show execution order: Shell hooks run BEFORE plugin hooks for same logical event (PreToolExecute shell runs at line 1757, then plugin ToolExecuteBefore at line 1770).

6. **Missing skill file**: Should have `.opencode/skills/hooks/SKILL.md` following the pattern of other modules.

## Recommendations

1. Add deprecation notice for InlineScript in config schema doc and architecture doc.
2. Create `.opencode/skills/hooks/SKILL.md` to document the hooks module.
3. Add sequence diagram showing hook execution order.
4. Document the specialized dispatch methods in PluginService.
5. Consider adding `HookRegistry::has_hooks()` to architecture doc or verifying it's used.

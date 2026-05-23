# Hooks Module Architecture Review

## Verification Results

### Claims

| Claim | Status | Evidence |
|-------|--------|----------|
| HookEvent enum: PreToolExecute, PostToolExecute, SessionStart, SessionEnd, AgentStart, AgentEnd | VERIFIED | src/hooks/mod.rs:17-24 |
| HookContext struct with event, session_id, tool_name, tool_arguments, tool_result, timestamp | VERIFIED | src/hooks/mod.rs:55-63 |
| HookRegistry with HashMap<HookEvent, Vec<Box<dyn Hook>>> | VERIFIED | src/hooks/mod.rs:150-152 |
| Hook trait: async fn execute(&self, ctx: &HookContext) -> Result<(), AppError> | VERIFIED | src/hooks/mod.rs:89-92 |
| run_hooks returns Vec<AppError>, collects errors not early-returned | VERIFIED | src/hooks/mod.rs:191-201 |
| ShellCommandHook struct with command, timeout, event fields | VERIFIED | src/hooks/mod.rs:94-98 |
| ShellCommandHook::new() with default 30 second timeout | VERIFIED | src/hooks/mod.rs:101-107 |
| ShellCommandHook uses user's actual PATH from environment | VERIFIED | src/hooks/mod.rs:118 |
| HookConfigEntry and HookConfig structs in config/schema.rs | VERIFIED | config/schema.rs:85-117 |
| InlineScript deprecated with warning | VERIFIED | src/hooks/mod.rs:181-183 |
| Integration point: SessionStart at loop.rs:1255 | VERIFIED | loop.rs:1255 |
| Integration point: AgentStart at loop.rs:1351 | VERIFIED | loop.rs:1351 |
| Integration point: AgentEnd at loop.rs:1524 | VERIFIED | loop.rs:1524 |
| Integration point: SessionEnd at loop.rs:1545 | VERIFIED | loop.rs:1545 |
| Integration point: PreToolExecute at loop.rs:1751 | VERIFIED | loop.rs:1751 |
| Integration point: PostToolExecute at loop.rs:1825 | VERIFIED | loop.rs:1825 |
| HookType enum: Auth, Provider, ToolDefinition, ToolExecuteBefore, ToolExecuteAfter, etc. | VERIFIED | src/plugin/hooks.rs:6-20 |
| HookResult struct with output, blocked, error | VERIFIED | src/plugin/hooks.rs:67-72 |
| HookResult::ok(), ::blocked(), ::error() constructors | VERIFIED | src/plugin/hooks.rs:74-97 |
| Two hook systems: src/hooks/mod.rs (shell) and src/plugin/hooks.rs (WASM) | VERIFIED | Verified both files exist |
| Shell command hooks never block | VERIFIED | No blocking mechanism in run_hooks |
| Plugin hooks can block (ToolExecuteBefore, SessionCompacting) | VERIFIED | loop.rs:1765-1768, 1207-1209 |
| Integration: SessionCompacting at loop.rs:1157 | VERIFIED | loop.rs:1198 (arch doc shows 1157) |
| Integration: ToolExecuteBefore at loop.rs:1764 | VERIFIED | loop.rs:1764 |
| Integration: ToolExecuteAfter at loop.rs:1806 | VERIFIED | loop.rs:1806 |
| PreAgentRun and PostAgentRun not implemented (documented but missing) | VERIFIED | No such variants in HookEvent |
| Stream errors break loop ensuring AgentEnd/SessionEnd hooks run | VERIFIED | AGENTS.md confirmed fix |

## Bugs Found

### Critical
None identified.

### High
None identified.

### Medium

1. **HookRegistry not thread-safe for mutation**: `HookRegistry` uses `HashMap` without synchronization. While current usage pattern (`&self` references) appears safe, if the registry were ever shared across async tasks or threads, there could be race conditions on `register()` calls. Consider using `Mutex` or `RwLock` if future sharing is intended.

2. **No ordering/priority for shell command hooks**: Hooks execute in registration order with no priority mechanism. If multiple hooks are registered for the same event, there's no way to control execution order. The plugin hooks system has `priority` field (`HookRegistration` at plugin/hooks.rs:104) but shell command hooks do not.

3. **Architecture doc line numbers are stale**: The integration table in architecture/hooks.md (lines 180-188, 193-198) shows line numbers that were accurate at time of writing but are now outdated (e.g., SessionCompacting shows line 1157 but actual is 1198). Line numbers drift with every code change.

## Improvement Suggestions

### Performance
None identified - the hook execution is straightforward and efficient.

### Correctness
1. **Consider configurable default timeout**: The 30-second default timeout in `ShellCommandHook::new()` is hardcoded. Consider making this configurable via `HookRegistry::new()` or a config setting.

2. **Hook error handling consistency**: Shell command hook errors are logged as `error!` level in loop.rs, while plugin hook errors are logged as `warn!` level (loop.rs:1771, 1808). Consider unifying the log level approach.

### Maintainability

1. **Remove hardcoded line numbers from architecture doc**: The integration table should describe logical locations (e.g., "before tool execution" or "in execute_tool_calls()") rather than line numbers which become stale.

2. **Add hook execution metrics**: No instrumentation for hook execution duration or success/failure rates. Consider adding tracing spans or metrics for observability.

3. **Hook retry mechanism missing**: If a hook fails due to transient conditions, there's no retry logic. Consider adding optional retry with backoff for shell command hooks.

4. **Missing PreAgentRun/PostAgentRun events**: These are documented but not implemented. Either implement them or remove from documentation to avoid confusion.

5. **No maximum hook count limit**: `run_hooks()` will execute all registered hooks for an event. If many hooks are registered, this could cause delays. Consider setting a reasonable maximum.

## Priority Actions (top 5 items to fix)

1. **Remove line numbers from architecture integration tables** - Replace with logical location descriptions to prevent future staleness.

2. **Add priority field to ShellCommandHook** - Mirror the `priority` field from plugin `HookRegistration` to allow ordering control.

3. **Make HookRegistry thread-safe** - Add `Mutex` wrapper to `hooks` HashMap for future-proofing against concurrent access.

4. **Implement PreAgentRun/PostAgentRun OR remove from docs** - Either add these missing HookEvent variants or clean up the documentation.

5. **Unify hook error log levels** - Change plugin hook error logs from `warn!` to `error!` to match shell command hook handling.
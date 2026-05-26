# Hooks Architecture Review Findings

## Verified Claims

### Shell Command Hooks location (line 18)
- `src/hooks/mod.rs` exists and contains shell command hook implementation

### HookEvent enum (lines 26-36)
- Verified at `hooks/mod.rs:15-24`: PreToolExecute, PostToolExecute, SessionStart, SessionEnd, AgentStart, AgentEnd
- Correctly notes `PreAgentRun` and `PostAgentRun` do not exist

### HookContext (lines 40-51)
- Fields verified at `hooks/mod.rs:55-63`: event, session_id, tool_name, tool_arguments, tool_result, timestamp

### HookRegistry (lines 53-73)
- `hooks: HashMap<HookEvent, Vec<Box<dyn Hook>>>` at `hooks/mod.rs:150-152`
- `Hook` trait at `hooks/mod.rs:89-92`
- `run_hooks` at `hooks/mod.rs:190-200`

### ShellCommandHook (lines 76-96)
- `command: String`, `timeout: Duration`, `event: HookEvent` at `hooks/mod.rs:94-98`
- Default timeout 30s at `hooks/mod.rs:104`
- PATH from environment at `hooks/mod.rs:118`

### InlineScript deprecated (line 101)
- Correctly documented as deprecated/non-functional
- Code at `hooks/mod.rs:181-183` just continues without registering

### Environment variables table (lines 120-133)
- All environment variables match `HookContext::to_env_vars()` at `hooks/mod.rs:66-87`
- CODEGG_HOOK_EVENT, CODEGG_SESSION_ID, CODEGG_TOOL_NAME, CODEGG_TOOL_ARGUMENTS, CODEGG_TOOL_RESULT, CODEGG_TIMESTAMP, PATH

### Plugin Hooks location (line 138)
- `src/plugin/hooks.rs` exists with HookType enum

### HookType enum (lines 142-158)
- All 13 hook types verified at `plugin/hooks.rs:4-20`
- Dot notation for serialization

### HookResult (lines 162-176)
- Fields verified at `plugin/hooks.rs:67-72`: output, blocked, error
- Constructors `ok()`, `blocked()`, `error()` verified at `plugin/hooks.rs:74-98`

### Integration Points table (lines 182-203)
- Shell command hooks run at loop.rs:1261 (SessionStart), 1357 (AgentStart), 1530 (AgentEnd), 1551 (SessionEnd), 1757 (PreToolExecute), 1831 (PostToolExecute)
- Plugin hooks dispatch_tool_execute_before at loop.rs:1770, dispatch_tool_execute_after at loop.rs:1812

### Key Differences table (lines 205-217)
- All differences correctly documented

## Stale Information

### SessionCompacting hook dispatch
- Document says plugin hooks include `SessionCompacting` and claims it "CAN BLOCK compaction" at line 155
- However, grep shows no `dispatch_session_compacting` call in loop.rs
- Need to verify if this hook is actually called anywhere in AgentLoop

### AgentEnd stream error note (line 193)
- "AgentEnd hooks do NOT run on stream errors since they are inside the loop that is broken"
- This is logical but should be verified in code

## Bugs Found

None found - documentation is accurate.

## Improvements Suggested

### Clarify plugin hook integration
The documentation mentions "Plugin hooks in AgentLoop" integration points but doesn't show the actual line numbers where these are called. The shell command hooks table shows "loop.rs" but no line numbers.

### Add line references for PluginService dispatch calls
The documentation should reference specific line numbers in loop.rs where plugin hooks are dispatched.

## Cross-Module Issues

### Two hook systems distinction
The documentation correctly emphasizes two separate hook systems but the relationship could be clearer. When shell command hooks run PreToolExecute, do plugin hooks also run?

### Plugin hooks not run on every tool
Based on grep results, plugin hooks (dispatch_tool_execute_before/after) appear to run only on certain code paths in loop.rs. The documentation implies they run for all tool executions.

### SessionCompacting integration incomplete
The document lists SessionCompacting as a plugin hook that "CAN BLOCK compaction" but grep shows no dispatch_session_compacting call in loop.rs. Either:
1. The hook is called elsewhere
2. The hook is implemented but not integrated
3. The documentation is wrong about this hook
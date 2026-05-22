# Hooks Module Override

This file contains hooks-specific guidance and overrides root AGENTS.md.

## Two Hook Systems

opencode-rs has two distinct hook systems:

1. **Shell Command Hooks** (`src/hooks/mod.rs`) - User-configured via config.yaml
2. **Plugin Hooks** (`src/plugin/hooks.rs`) - WASM plugins with HookType enum

## Critical Implementation Notes

### ToolExecuteBefore/After Now Called (2026-05-22)

Plugin `ToolExecuteBefore` and `ToolExecuteAfter` hooks are now invoked from `execute_tool_calls()` in `agent/loop.rs`. Previously these hooks existed but were never called.

- `dispatch_tool_execute_before()` is called before each tool execution
- `dispatch_tool_execute_after()` is called after each tool execution
- If `ToolExecuteBefore` returns `blocked: true`, tool execution is aborted with `ToolError::Execution("blocked by plugin hook")`

### Configuration Validation Added (2026-05-22)

`HookRegistry::from_config()` now logs warnings for:
- Invalid event names (e.g., typos like `"pre_tool_execut"`)
- Unimplemented `InlineScript` type (with `#[deprecated]` attribute)

### Env Vars Use CODEGG_ Prefix

Shell hooks use `CODEGG_HOOK_EVENT`, `CODEGG_SESSION_ID`, etc. (not `OPENCODE_`).

## Plugin Service Dispatch

The `PluginService` in `src/plugin/service.rs` dispatches hooks with:
- 5 second timeout (hardcoded)
- Error messages include plugin_id: `"plugin:codex: hook timeout: ..."`

## InlineScript Deprecated

`InlineScript` variant in `HookConfig` is deprecated with message: "InlineScript is not implemented. Use ShellCommand instead." Users configuring this will see compiler warnings.

## Hook Execution Order in Tool Calls

When executing tools, hooks run in this order:
1. Shell `PreToolExecute` hooks
2. Plugin `ToolExecuteBefore` hooks (can block)
3. Tool execution
4. Plugin `ToolExecuteAfter` hooks
5. Shell `PostToolExecute` hooks

## Related Skills

- `.opencode/skills/plugin/SKILL.md` - Plugin hooks system
- `.opencode/skills/hooks/SKILL.md` - Detailed hooks guide
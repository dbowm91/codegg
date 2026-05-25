# Hooks Architecture Review (2026-05-25)

## Verified Correct Items

### Shell Command Hooks (src/hooks/mod.rs)
- **HookEvent enum** (lines 17-24): All 6 variants correct - `PreToolExecute`, `PostToolExecute`, `SessionStart`, `SessionEnd`, `AgentStart`, `AgentEnd`
- **HookEvent::as_str()** (lines 27-36): Returns underscore notation (`pre_tool_execute`)
- **HookContext struct** (lines 55-63): All fields match doc
- **HookRegistry struct** (line 151): `HashMap<HookEvent, Vec<Box<dyn Hook>>>` - correct
- **Hook trait** (lines 89-92): `async fn execute(&self, ctx: &HookContext) -> Result<(), AppError>` - correct
- **ShellCommandHook** (lines 94-98): `command`, `timeout`, `event` fields - correct
- **run_hooks()** (lines 191-201): Returns `Vec<AppError>`, collects all errors - correct
- **to_env_vars()** (lines 66-87): Produces correct env vars including `CODEGG_HOOK_EVENT`, `CODEGG_SESSION_ID`, etc.

### Plugin Hooks (src/plugin/hooks.rs)
- **HookType enum** (lines 4-20): All 14 variants correct
- **HookType::as_str()** (lines 22-39): Returns dot notation (`tool.execute.before`) - correct
- **HookResult** (lines 67-97): `output`, `blocked`, `error` fields with helpers - correct

### AgentLoop Integration (src/agent/loop.rs)
- **SessionStart** (lines 1249-1264): Outside loop, before `apply_auto_routing` - correct
- **AgentStart** (lines 1345-1360): Inside loop at line 1313 - correct
- **PreToolExecute shell hooks** (lines 1744-1762): Before plugin `dispatch_tool_execute_before` - correct
- **Plugin ToolExecuteBefore** (lines 1764-1779): Can block via `blocked: true` check - correct
- **Plugin ToolExecuteAfter** (lines 1805-1816): After tool execution - correct
- **PostToolExecute shell hooks** (lines 1818-1836): After plugin hook - correct
- **AgentEnd** (lines 1518-1533): Inside loop, skipped on stream error break at line 1369 - correct
- **SessionEnd** (lines 1539-1554): Outside loop, after `drain_follow_up` at line 1536 - correct
- **SessionCompacting** (lines 1157-1204): Called when `needs_compaction()` returns true - correct

### Configuration (src/config/schema.rs)
- **HookConfigEntry** (lines 84-89): `event: String`, `hook: HookConfig` - correct
- **HookConfig enum** (lines 103-117): `ShellCommand { command, timeout_secs }`, deprecated `InlineScript` - correct

### Key Differences (Architecture Doc)
- **Shell vs Plugin notation**: Shell uses underscore (`pre_tool_execute`), Plugin uses dot (`tool.execute.before`) - verified correct
- **Blocking behavior**: Shell hooks never block, Plugin `ToolExecuteBefore` and `SessionCompacting` can block - verified correct
- **Error format**: Plugin error includes `plugin_id: {plugin_id}: hook timeout: ...` - verified correct

## Incorrect/Stale Items

### architecture/hooks.md:38
**Issue**: States `PreAgentRun` and `PostAgentRun` are "documented but not implemented"
**Reality**: These events don't exist anywhere - they're not documented in code or planned
**Fix**: Remove the note entirely, or clarify "These events do not exist"

### .opencode/skills/hooks/SKILL.md:149-165
**Issue**: YAML configuration example uses a map-based format that doesn't match actual config schema
**Reality**: Config schema uses `hooks: Vec<HookConfigEntry>` (array), not a map keyed by event name
**Fix**: Update to show correct TOML format matching architecture/hooks.md:101-116

## No Bugs Found in Related Code

The hook integration in `agent/loop.rs` is correctly implemented:
- Shell hooks run before plugin hooks for tool execution
- Plugin hooks can block tool execution
- Stream errors correctly break the loop before AgentEnd hooks
- SessionEnd hooks correctly run after loop exit

## Line Numbers Needing Updates

| File | Line(s) | Issue |
|------|---------|-------|
| `architecture/hooks.md` | 38 | Remove stale PreAgentRun/PostAgentRun note |
| `.opencode/skills/hooks/SKILL.md` | 149-165 | Fix YAML config example to use TOML array format |

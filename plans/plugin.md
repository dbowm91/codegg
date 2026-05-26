# Plugin Architecture Review Findings

## Verified Claims

### Structure (lines 24-43)
- All files listed in project structure exist at `src/plugin/`
- `mod.rs` exports match actual exports
- `loader.rs`, `hooks.rs`, `registry.rs`, `manifest.rs`, `service.rs`, `install.rs`, `api.rs`, `tui.rs`, `event_bus.rs`, `marketplace.rs` all present

### HookType Enum (lines 85-101)
- All 13 hook types verified: Auth, Provider, ToolDefinition, ToolExecuteBefore, ToolExecuteAfter, ChatParams, ChatHeaders, Event, Config, ShellEnv, TextComplete, SessionCompacting, MessagesTransform
- `as_str()` returns dot notation (verified at `hooks.rs:23-39`)
- `parse()` exists (verified at `hooks.rs:41-58`)

### HookContext/HookResult (lines 104-127)
- Fields match actual code at `hooks.rs:61-72`

### ModuleCache (lines 141-156)
- Struct fields verified at `loader.rs:110-115`
- Methods: `get_or_compile`, `get_plugin_fuel`, `reserve_fuel`, `return_fuel` all exist and match

### PluginService (lines 188-207)
- `hook_timeout: Duration` default 5 seconds verified at `service.rs:18`
- All dispatch methods verified at `service.rs:143-246`

### PluginRegistry (lines 210-233)
- `PluginInfo` fields verified at `registry.rs:8-15`
- All methods exist: `register`, `unregister`, `hooks_for`, `is_enabled`, `set_enabled`

### install.rs (lines 235-243)
- `plugins_dir()` returns `~/.local/share/codegg/plugins` via `dirs::data_local_dir()` verified at `install.rs:23-28`
- All functions exist: `install_from_path`, `install_from_url`, `uninstall`

### Builtin plugins (lines 268-299)
- BUILTIN_HANDLERS with copilot, gitlab, codex, poe verified at `builtin/mod.rs:18-39`
- `builtin_hook_handler` exists at `builtin/mod.rs:86-93`
- `register_builtins` exists at `builtin/mod.rs:50-76`

### MarketplaceService (lines 300-313)
- `list_official_plugins()` returns empty Vec (TODO) verified at `marketplace.rs:104-106`
- `list_repository_plugins()` returns empty Vec (TODO) verified at `marketplace.rs:108-110`

### Constants (lines 388-396)
- MAX_WASM_SIZE = 10MB at `loader.rs:10`
- WASM_FUEL_PER_HOOK = 1_000_000 at `loader.rs:12`
- WASM_HOOK_TIMEOUT = 30s at `loader.rs:14`
- MAX_PLUGIN_FUEL_BUDGET = 10_000_000 at `loader.rs:16`

### Security (lines 404-413)
- Symlink validation in archives at `install.rs:142-147`
- Path traversal validation at `install.rs:149-156`

### Plugin IDs (lines 423-427)
- `plugin:{name}` and `builtin:{name}` format verified at `service.rs:32`

## Stale Information

### Hook dispatch timeout (line 407)
- Documentation says "5s per hook dispatch, 30s for WASM execution"
- `hook_timeout` is indeed 5s (`service.rs:18`)
- WASM_HOOK_TIMEOUT is 30s (`loader.rs:14`)
- However, the doc says "30s for WASM execution" but the `PluginService::execute_hook_with_timeout` uses `self.hook_timeout` (5s) not WASM_HOOK_TIMEOUT (30s). The 30s timeout is only for the inner WASM execution loop (`loader.rs:289`).
- This is slightly misleading - the outer hook dispatch has 5s timeout, inner WASM execution has 30s.

### Fuel logic documentation (lines 398-403)
- "Budget exhausted → returns `HookResult::ok(ctx.input)` early" is accurate
- However the doc doesn't mention that fuel is NOT returned when we exit early due to metadata read failures, etc.

## Bugs Found

### Fuel leak on metadata read failure (`loader.rs:255-261`)
When `std::fs::metadata(&wasm_path)` fails after fuel has been reserved, the function returns `HookResult::ok(ctx.input)` WITHOUT calling `module_cache::CACHE.return_fuel(plugin_id, fuel_reserved)`. This causes fuel to be permanently lost from the plugin's budget.

```rust
let metadata = match std::fs::metadata(&wasm_path) {
    Ok(m) => m,
    Err(e) => {
        tracing::warn!(...);
        return HookResult::ok(ctx.input);  // BUG: fuel leak!
    }
};
```

**Same issue at lines 263-271** - size check also leaks fuel.

Compare with proper handling at lines 327-328, 336-340, etc. where `return_fuel` is called before returning.

### MarketplaceService fields incomplete
`marketplace.rs:33-35` shows:
```rust
pub struct MarketplaceService {
    plugins_dir: PathBuf,
}
```
But `new()` and `plugins_dir()` methods access only `plugins_dir`. The marketplace service doesn't store other configuration that might be expected.

## Improvements Suggested

### Documentation inconsistency on hook timeout
The 5s vs 30s timeout distinction should be clarified. The outer `execute_hook_with_timeout` uses 5s, but the inner WASM execution uses 30s.

### InlineScript deprecation note (line 101-102)
Correctly documents InlineScript as deprecated, but the actual skip in `hooks/mod.rs:181-183` just `continue`s without logging. Could add a warn! for visibility.

### Missing `dispatch_provider` in PluginService
The doc at line 203 shows `dispatch_provider` as one of the dispatch methods, and it actually exists at `service.rs:239-245`. This is correct but the doc only shows "... other hook types" at line 206.

## Cross-Module Issues

### Shell command hooks vs Plugin hooks naming conflict
The hooks module has `HookEvent` (`PreToolExecute`, etc.) while plugin module has `HookType` (`ToolExecuteBefore`, etc.). The architecture document correctly distinguishes these, but code could be confusing.

### AgentLoop integration points
The hook flow diagram (lines 349-382) references `AgentLoop (or other component)` but doesn't specify exact code locations. The integration points are documented in `hooks.md` but the plugin.md doesn't cross-reference back to show which AgentLoop methods call plugin hooks.
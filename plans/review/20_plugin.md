# Plugin Architecture Review (20_plugin.md)

## Verified Correct Items

### Core Types (hooks.rs, manifest.rs)
- **HookType enum**: All 13 variants match (`Auth`, `Provider`, `ToolDefinition`, `ToolExecuteBefore`, `ToolExecuteAfter`, `ChatParams`, `ChatHeaders`, `Event`, `Config`, `ShellEnv`, `TextComplete`, `SessionCompacting`, `MessagesTransform`)
- **HookType::as_str()**: Returns dot notation (e.g., `tool.execute.before`) - matches implementation
- **HookType::parse()**: Correctly parses dot notation strings
- **HookContext/HookResult**: Structs match with correct fields
- **HookResult::ok()/blocked()/error()**: Implementations match
- **HookRegistration**: Struct with `plugin_id`, `hook_type`, `priority` matches
- **PluginManifest/HookSpec**: All fields match

### Module Organization (mod.rs)
- All exports verified: `api`, `builtin`, `event_bus`, `hooks`, `install`, `loader`, `manifest`, `registry`, `service`, `tui`
- All re-exports verified correct

### Loader (loader.rs)
- **LoadedPlugin**: Struct matches with `manifest`, `wasm_path`, `plugin_dir`
- **Fuel constants**: All correct (`MAX_WASM_SIZE=10MB`, `WASM_FUEL_PER_HOOK=1_000_000`, `WASM_HOOK_TIMEOUT=30s`, `MAX_PLUGIN_FUEL_BUDGET=10_000_000`, `FUEL_RESET_INTERVAL_SECS=60`)
- **execute_wasm_hook()**: Signature matches, feature-gated correctly
- **ModuleCache**: Private module with `get_or_compile()`, `get_plugin_fuel()`, `reserve_fuel()`, `return_fuel()`, `stats()` - all correct

### Service (service.rs)
- **PluginService**: Struct with `registry`, `hook_timeout` (default 5s)
- **Methods**: `new()`, `with_hook_timeout()`, `registry()`, `load_and_register()`, `dispatch_hook()`
- **All 13 dispatch methods**: `dispatch_auth`, `dispatch_provider`, `dispatch_tool_definition`, `dispatch_tool_execute_before`, `dispatch_tool_execute_after`, `dispatch_chat_params`, `dispatch_chat_headers`, `dispatch_event`, `dispatch_config`, `dispatch_shell_env`, `dispatch_text_complete`, `dispatch_session_compacting`, `dispatch_messages_transform`
- **execute_hook_with_timeout()**: Correctly distinguishes `builtin:` vs `plugin:` prefixes

### Registry (registry.rs)
- **PluginInfo/PluginRegistry**: Structs match with `plugins` and `hooks` RwLock fields
- **register/unregister/hooks_for/is_enabled/set_enabled**: All match
- **sort_hooks()**: Private helper called after each `register()` - correctly sorts by priority

### Install (install.rs)
- **plugins_dir()**: Returns `~/.local/share/codegg/plugins` via `dirs::data_local_dir()` - correct
- **install_from_path/install_from_url/uninstall**: Signatures and behavior match
- **Symlink protection**: `copy_dir_all()` and `extract_plugin_archive()` both reject symlinks
- **Path traversal protection**: Archive extraction validates paths stay within destination

### Builtin Plugins (builtin/mod.rs, copilot.rs, gitlab.rs, codex.rs, poe.rs)
- **BuiltinPlugin struct**: Matches with `manifest` and `handler` fields
- **BUILTIN_HANDLERS**: LazyLock/RwLock HashMap with all 4 handlers (copilot, gitlab, codex, poe)
- **builtin_hook_handler()**: Correctly dispatches to registered handlers
- **register_builtins()**: Async function that registers all builtins
- **Auth injection**: All 4 builtins inject `Bearer {token}` into Authorization header

### Marketplace (marketplace.rs)
- **MarketplaceService**: Struct with `plugins_dir` field
- **list_local_plugins()**: Async function that scans plugins directory
- **search_plugins()**: Filters local plugins by name/description
- **list_official_plugins/list_repository_plugins**: Return empty Vec (TODO stubs)

### Event Bus (event_bus.rs)
- **PluginEventBus/PluginEventSubscription**: Match implementation
- **publish()**: Correctly logs events and matches subscriptions without routing to plugins

### TUI (tui.rs)
- **TuiPluginRegistry**: Struct with `routes`, `components`, `plugin_configs` - matches
- **TuiRoute/TuiComponent**: Structs match with correct fields

### API (api.rs)
- **ApiVersion/Stability**: Match implementation
- **api::hooks::HookType**: Duplicate of main HookType (internal API types)
- **tools/provider modules**: ToolDefinition, ToolInput, ToolOutput, ChatRequest, Message, etc.

## Incorrect/Stale Items

### 1. `sort_hooks()` sorting documentation (line 360)
**Issue**: Architecture says "hooks sorted by priority at registration time" - this is correct, but `sort_hooks()` is called in `register()` after each insert, not batched.

**Fix**: Already correct, no change needed. Clarify that sorting happens after each `register()` call.

### 2. `dispatch_to_plugin` referenced but removed
**Issue**: Line 371 in "Hook Flow" diagram references `dispatch_to_plugin` which was removed in a previous review session. This was dead code that only logged and never actually dispatched.

**Fix**: Update diagram to show `execute_hook_with_timeout()` directly.

### 3. `check_and_reset_fuel_budget()` never called
**Issue**: `loader.rs:24-41` defines `check_and_reset_fuel_budget()` which resets the global `PLUGIN_FUEL_BUDGET` every 60 seconds, but this function is never called in `execute_wasm_hook()`. The global `PLUGIN_FUEL_BUDGET` serves no purpose since only per-plugin fuel in `ModuleCache` is used.

**Fix**: Either remove the dead code or document that global fuel budget is a planned feature not yet implemented.

### 4. `PluginRegistry::sort_hooks()` missing from documentation
**Issue**: The private `sort_hooks()` helper method is not documented, but it's important for understanding hook ordering.

**Fix**: Add to `registry.rs` documentation: "Hooks are sorted by priority after each registration via private `sort_hooks()` method."

## Bugs Found in Related Code

### 1. Fuel leak when WASM hook function not found (loader.rs:344-354)
**Bug**: When `instance.get_func()` returns `None` (hook function not found), the code returns `(HookResult::ok(ctx.input), 0)` but the reserved fuel is NOT returned via `return_fuel()`. This causes a fuel leak on every call to a plugin that doesn't implement a specific hook.

```rust
// Line 344-354
let func = match instance.get_func(&mut store, func_name) {
    Some(f) => f,
    None => {
        tracing::debug!(...);  // Returns early WITHOUT returning fuel!
        return Ok::<(HookResult, u64), BoxError>((HookResult::ok(ctx.input), 0));
    }
};
```

**Fix**: Change to return remaining fuel instead of 0, or add fuel return before the early return.

### 2. Fuel leak on other early errors after fuel reserved (loader.rs:356-409)
**Bug**: After `fuel_reserved` is set (line 270), several early returns don't return the fuel:
- Line 359-365: `get_memory()` returns `None`
- Line 373-379: `get_func("allocate")` returns `None`
- Line 389-396: `alloc_func.call()` returns no value
- Line 403-409: Input exceeds memory bounds

All these cases return `(HookResult::error(...), 0)` without calling `module_cache::CACHE.return_fuel()`.

**Fix**: Add fuel return before each early error return.

### 3. Global `PLUGIN_FUEL_BUDGET` never decremented
**Bug**: The global `PLUGIN_FUEL_BUDGET` (line 15) is only checked at line 262-266 but is never decremented during execution. Only per-plugin fuel in `ModuleCache` is actually used.

**Fix**: Either integrate global budget checking/resetting into `execute_wasm_hook()` or remove the dead global budget code.

## Line Number Corrections

| Line | Issue | Fix |
|------|-------|-----|
| 371 | "dispatch_to_plugin" doesn't exist | Change to "execute_hook_with_timeout" |
| 383-406 | Global budget code never called | Document as planned/incomplete or remove |

## Summary

The architecture document is **90% accurate**. Main issues:
1. Dead `check_and_reset_fuel_budget()` function never called
2. `dispatch_to_plugin` referenced but removed
3. Multiple fuel leak bugs in `execute_wasm_hook()` error paths
4. Global `PLUGIN_FUEL_BUDGET` is dead code

The SKILL.md (v1.1.0) is more accurate and up-to-date than the architecture doc.

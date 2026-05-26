# Plugin Architecture Review

## Summary
The plugin architecture document is mostly accurate. Key verified items include the HookType enum, fuel tracking constants, PluginService structure, and builtin plugin handlers. However, there are discrepancies around the plugins_dir path (platform-dependent vs Linux-specific in doc), dead code for check_and_reset_fuel_budget(), and stale security documentation.

## Verified Correct
- **HookType enum**: `src/plugin/hooks.rs:4-20` - All 13 hook types present (Auth, Provider, ToolDefinition, ToolExecuteBefore, ToolExecuteAfter, ChatParams, ChatHeaders, Event, Config, ShellEnv, TextComplete, SessionCompacting, MessagesTransform)
- **Fuel constants**: `src/plugin/loader.rs:10-21` - MAX_WASM_SIZE (10MB), WASM_FUEL_PER_HOOK (1M), WASM_HOOK_TIMEOUT (30s), MAX_PLUGIN_FUEL_BUDGET (10M), FUEL_RESET_INTERVAL_SECS (60s)
- **PluginService**: `src/plugin/service.rs:9-19` - registry, hook_timeout (default 5s) match doc
- **PluginRegistry**: `src/plugin/registry.rs:17-19` - plugins and hooks RwLock<HashMap<Vec>> structure
- **BuiltinPlugin struct**: `src/plugin/builtin/mod.rs:13-16` - manifest and handler fields present
- **BUILTIN_HANDLERS**: `src/plugin/builtin/mod.rs:18-39` - copilot, gitlab, codex, poe handlers registered
- **HookResult methods**: `src/plugin/hooks.rs:74-98` - ok(), blocked(), error() implementations
- **ModuleCache**: `src/plugin/loader.rs:135-140` - modules, hits, misses, fuel_budgets fields match
- **Hook flow**: `src/plugin/service.rs:63-114` and `loader.rs:248-541` - dispatch_hook → execute_hook_with_timeout → WASM/builtin correctly implemented

## Discrepancies Found
- **plugins_dir path**: `src/plugin/install.rs:23-28` uses `dirs::data_local_dir()/codegg/plugins` which is platform-dependent. Doc shows `~/.local/share/codegg/plugins/` (Linux-specific). On macOS this would be `~/Library/Application Support/codegg/plugins`. Architecture doc should clarify this is platform-dependent.
- **Fuel tracking logic inverted**: Doc states "Budget exhausted → returns `HookResult::ok(ctx.input)` early" implying line 262-266 handles exhaustion, but `current_plugin_fuel >= MAX_PLUGIN_FUEL_BUDGET` means fuel is NOT exhausted (it's at or above max). The condition actually triggers when fuel is already maxed (unspent budget), not when exhausted.

## Bugs Identified
- **Dead code - check_and_reset_fuel_budget()**: `src/plugin/loader.rs:24-41` - This function is never called anywhere in the codebase. PLUGIN_FUEL_BUDGET and PLUGIN_FUEL_LAST_RESET statics exist but the reset logic is unreachable. Per AGENTS.md: "Plugin global fuel budget unused - dead code that could be removed or integrated."

## Stale Items in Architecture Doc
- **Security section "Path Traversal"**: Doc claims "Archive extraction validates paths" but `extract_plugin_archive()` at `src/plugin/install.rs:128-162` only checks path canonicalization. However, line 149 `let dst_path = dest.join(&entry_path)` followed by canonicalization check at 150-156 does validate destination. The doc is mostly correct but could be clearer.
- **Symlink check during install**: `copy_dir_all()` at `src/plugin/install.rs:179-213` properly rejects symlinks at line 191-196, but the doc at "Symlinks: Not allowed" is accurate.

## Improvement Suggestions
- **Remove dead code**: Consider removing `check_and_reset_fuel_budget()`, `PLUGIN_FUEL_BUDGET`, `PLUGIN_FUEL_LAST_RESET`, and `MAX_PLUGIN_FUEL_BUDGET` if the global fuel budget is not being used (only per-plugin fuel in ModuleCache is used)
- **Clarify plugins_dir platform dependency**: The doc should note that `plugins_dir()` uses `dirs::data_local_dir()` which resolves to platform-specific locations
- **Add missing dispatch method**: `dispatch_provider()` exists at `src/plugin/service.rs:239-245` but isn't explicitly documented in the Hook Flow section

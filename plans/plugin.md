# Plugin Architecture Review

## Architecture Document
- Path: architecture/plugin.md

## Source Code Location
- src/plugin/

## Verification Summary
Pass

## Verified Claims (table format)

| Claim | Status | Notes |
|-------|--------|-------|
| Location: src/plugin/ | Pass | Correct |
| WASM plugin loading via Wasmtime (feature-gated `plugins`) | Pass | Cargo.toml line 171 confirms `plugins = ["wasmtime", "wasmtime-wasi"]` |
| Project structure (mod.rs, loader.rs, hooks.rs, etc.) | Pass | All files present as documented |
| HookType enum with all variants | Pass | hooks.rs has all 12 hook types |
| HookType::as_str() returns dot notation | Pass | Returns "tool.execute.before" etc. |
| HookType::parse() parses dot notation | Pass | Confirmed in hooks.rs |
| HookResult::ok(), blocked(), error() | Pass | All present in hooks.rs |
| LoadedPlugin struct | Pass | loader.rs:58-62 matches |
| PluginService struct with hook_timeout | Pass | service.rs:9-12, default 5s |
| PluginRegistry with plugins and hooks | Pass | registry.rs:17-20, uses RwLock |
| PluginRegistry::hooks_for() sorts by priority | Pass | registry.rs:85-87 calls sort_hooks() after registration |
| PluginInfo struct | Pass | registry.rs:8-15 matches |
| install_from_path(), install_from_url(), uninstall() | Pass | install.rs exports all three |
| plugins_dir() returns ~/.local/share/codegg/plugins | Pass | install.rs:23-28 |
| TuiPluginRegistry, TuiRoute, TuiComponent | Pass | tui.rs matches exactly |
| MarketplaceService with list_local/search | Pass | marketplace.rs:48-102 |
| BUILTIN_HANDLERS with copilot/gitlab/codex/poe | Pass | builtin/mod.rs:18-39 |
| Built-in plugin auth hook injection | Pass | All 4 builtins inject Bearer tokens |
| Hook Flow (dispatch_tool_execute_before) | Pass | Matches service.rs dispatch logic |
| Fuel tracking constants | Pass | loader.rs:10-21 constants present |
| ModuleCache with DashMap | Pass | loader.rs:135-235 |
| WASM plugin contract (memory, allocate, hooks) | Pass | loader.rs:356-510 |
| 10MB max WASM module size | Pass | loader.rs:10, 288 |
| 10MB max WASM output size | Pass | loader.rs:442 |
| 30s WASM hook timeout | Pass | loader.rs:14 |
| 5s hook dispatch timeout | Pass | service.rs:18 |
| Path traversal protection in archives | Pass | install.rs:136-156 |
| Symlinks not allowed | Pass | install.rs:142-146, 191-195 |
| Plugin IDs: plugin:{name}, builtin:{name} | Pass | service.rs:124, builtin/mod.rs:79 |
| dispatch_auth, dispatch_tool_execute_before, etc. | Pass | service.rs:143-245 has all dispatch methods |

## Issues Found

### Bugs

1. **Duplicate HookType definitions**: The codebase has two HookType enums:
   - `src/plugin/hooks.rs` (main one used throughout)
   - `src/plugin/api.rs::hooks::HookType` (seems unused, not exported from plugin module)
   
   The api.rs version is never used anywhere - it's an internal module. This is minor duplication but not a bug since the main types work correctly.

2. **register_builtins() re-registers handlers redundantly**: In builtin/mod.rs:50-76, `register_builtins()` first creates registrations and then calls `register_builtin_handler()` separately. However, the BUILTIN_HANDLERS LazyLock already pre-populates all handlers at initialization (lines 18-39), so the per-plugin `register_builtin_handler()` calls are redundant but harmless.

### Inconsistencies

1. **Feature flag naming in docs vs Cargo.toml**: The architecture document states `plugins` feature flag. Cargo.toml confirms this is correct. No issue.

2. **Module cache documentation**: The architecture doc shows `module_cache` as a nested module with `pub struct ModuleCache`. In the source, it's a private module (`#[cfg(feature = "plugins")] mod module_cache`) with a static `CACHE` instance. The public API is just the `execute_wasm_hook()` function. This is a documentation style difference, not an inconsistency.

### Missing Documentation

1. **MarketplaceService::list_official_plugins() returns empty**: This method is documented as TODO (returns `Vec::new()`), but this is not noted in the architecture doc. Should note it's not yet implemented.

2. **BuiltinPlugin struct**: The `BuiltinPlugin` struct in builtin/mod.rs:13-16 is used by `get_builtin_plugins()` but not documented in architecture. It contains `manifest` and `handler` fields.

3. **PluginEventBus** is documented but it's actually a dead/simple implementation. The architecture shows it as part of the hook flow but it's not actually used in the dispatch logic - it's just storing events. The `dispatch_to_plugin` function that was mentioned in the AGENTS.md notes was already removed.

4. **extract_plugin_archive() path validation**: The security section mentions "Path Traversal" protection but doesn't detail that it uses `canonicalize()` to verify paths stay within destination.

5. **install_from_url() supports both .wasm and .tar.gz**: Architecture only mentions `.wasm` in the example but the code handles both.

### Improvement Opportunities

1. **Dead code in api.rs**: The `api.rs::hooks` module defines duplicate types that are never used. Could consider removing or documenting why they exist.

2. **Missing `unregister()` async method in PluginRegistry**: While `unregister()` exists, it's not used anywhere in the codebase (no callers found). Could consider if it's needed.

3. **Builtin plugins only support Auth hook**: All four builtins (copilot, gitlab, codex, poe) only implement the Auth hook type, but the architecture shows them generically without specifying this limitation.

4. **No actual plugin marketplace**: `list_official_plugins()` and `list_repository_plugins()` return empty vectors - no actual marketplace integration exists.

## Recommendations

1. Document that MarketplaceService is not yet implemented for official/repository plugins
2. Add note about `BuiltinPlugin` struct in the architecture doc
3. Consider removing unused `api.rs::hooks` module or documenting its purpose
4. The architecture is generally accurate and well-maintained

---

*Review date: 2026-05-23*

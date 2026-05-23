# Plugin Module Architecture Review

**Review Date**: 2026-05-23
**Reviewed Files**: `architecture/plugin.md`, `src/plugin/*.rs`, `.opencode/skills/plugin/SKILL.md`

---

## Verified Claims (what matches)

### Project Structure
All files listed in architecture doc exist and match exactly:
- `mod.rs`, `loader.rs`, `hooks.rs`, `registry.rs`, `manifest.rs`
- `service.rs`, `install.rs`, `api.rs`, `tui.rs`, `event_bus.rs`, `marketplace.rs`
- `builtin/mod.rs`, `builtin/copilot.rs`, `builtin/gitlab.rs`, `builtin/codex.rs`, `builtin/poe.rs`

### Key Types - All Match

| Type | Location | Status |
|------|----------|--------|
| `PluginManifest` | `manifest.rs:5-16` | ✓ Exact match |
| `HookSpec` | `manifest.rs:18-23` | ✓ Exact match |
| `HookType` | `hooks.rs:4-20` | ✓ All 13 variants match |
| `HookContext` | `hooks.rs:61-65` | ✓ Exact match |
| `HookResult` | `hooks.rs:67-98` | ✓ Exact match (including `ok()`, `blocked()`, `error()`) |
| `HookRegistration` | `hooks.rs:100-105` | ✓ Exact match |
| `HookType::as_str()` | `hooks.rs:22-39` | ✓ Returns dot notation |
| `HookType::parse()` | `hooks.rs:41-58` | ✓ Parses dot notation |
| `LoadedPlugin` | `loader.rs:58-62` | ✓ Exact match |
| `PluginInfo` | `registry.rs:8-15` | ✓ Exact match |
| `PluginRegistry` | `registry.rs:17-19` | ✓ Exact match |
| `TuiRoute` | `tui.rs:6-12` | ✓ Exact match |
| `TuiComponent` | `tui.rs:14-19` | ✓ Exact match |
| `TuiPluginRegistry` | `tui.rs:21-25` | ✓ Exact match |
| `PluginManifest` | `marketplace.rs:21-31` | ✓ Exact match |
| `MarketplaceService` | `marketplace.rs:33-35` | ✓ Exact match |

### Feature Flag
- `plugins` feature correctly gates WASM-related code (not `plugin`) ✓

### Module Cache
- `ModuleCache` struct with `modules`, `hits`, `misses`, `fuel_budgets` fields ✓
- `get_or_compile()`, `get_plugin_fuel()`, `reserve_fuel()`, `return_fuel()` methods ✓

### Fuel Constants
- `MAX_WASM_SIZE = 10 * 1024 * 1024` (10MB) ✓
- `WASM_FUEL_PER_HOOK = 1_000_000` ✓
- `WASM_HOOK_TIMEOUT = 30 seconds` ✓
- `MAX_PLUGIN_FUEL_BUDGET = 10_000_000` ✓
- `FUEL_RESET_INTERVAL_SECS = 60` ✓

### Built-in Plugin Handler Registration
- `BUILTIN_HANDLERS` static with `copilot`, `gitlab`, `codex`, `poe` ✓
- `builtin_hook_handler()` function ✓
- `register_builtins()` async function ✓

### Security Features
- Symlink checking in `install_from_path` and `extract_plugin_archive` ✓
- Path traversal validation (canonicalize check) ✓
- WASM size limit (10MB) ✓
- Output size limit (10MB) ✓
- Hook timeout (5s per hook dispatch, 30s for WASM execution) ✓

### Hook Dispatch Methods in PluginService
All dispatch methods present:
- `dispatch_auth()`, `dispatch_provider()`, `dispatch_tool_definition()`
- `dispatch_tool_execute_before()`, `dispatch_tool_execute_after()`
- `dispatch_chat_params()`, `dispatch_chat_headers()`
- `dispatch_event()`, `dispatch_config()`, `dispatch_shell_env()`
- `dispatch_text_complete()`, `dispatch_session_compacting()`, `dispatch_messages_transform()`

### WASM Plugin Contract
- `memory` export required ✓
- `allocate(ptr, len) -> ptr` function required ✓
- `deallocate(ptr, len)` optional ✓
- `WasmHookResponse` format `{"output": {...}, "blocked": bool, "error": string}` ✓

---

## Bugs/Discrepancies Found

### BUG 1 (HIGH): WASM Path Construction Uses Wrong Directory and Prefix

**Location**: `loader.rs:276-278`

**Problem**: The WASM path is constructed incorrectly in two ways:

1. **Wrong prefix**: Uses `plugins/{plugin_id}/` instead of the actual install path from `install::plugins_dir()` which is `~/.local/share/codegg/plugins/`
2. **Wrong plugin_id format**: The `plugin_id` includes `plugin:` prefix (e.g., `plugin:my-plugin`), so path becomes `plugins/plugin:my-plugin/plugin.wasm` instead of `plugins/my-plugin/plugin.wasm`

```rust
// Current code (loader.rs:276-278):
let plugin_dir = format!("plugins/{}", plugin_id);
let wasm_path: PathBuf = format!("{}/plugin.wasm", plugin_dir).into();
```

**Expected path**: `~/.local/share/codegg/plugins/my-plugin/plugin.wasm`
**Actual path**: `plugins/plugin:my-plugin/plugin.wasm` (relative to CWD)

**Root Cause**: The `execute_wasm_hook` function was likely written for a development/setup context where plugins are in a local `plugins/` directory, but never updated to use the actual `install::plugins_dir()` path.

**Fix Required**:
- Option A: Use `install::plugins_dir()` and strip `plugin:` prefix from `plugin_id`
- Option B: Accept a `plugin_name` parameter (without prefix) instead of `plugin_id`

**Impact**: All WASM plugins fail to execute because `std::fs::metadata()` fails to find the WASM file.

---

### BUG 2 (MEDIUM): BuiltinPlugin struct not exported

**Location**: `builtin/mod.rs:13-16` and `mod.rs`

**Problem**: `BuiltinPlugin` struct is defined but not exported from `src/plugin/mod.rs`. The architecture doc shows this struct as part of the built-in plugin system.

```rust
// In builtin/mod.rs:
pub struct BuiltinPlugin {
    pub manifest: PluginManifest,
    pub handler: fn(HookContext) -> HookResult,
}
```

**Fix**: Add to `mod.rs` exports or mark as doc-hidden if intentionally internal.

---

### BUG 3 (LOW): MarketplaceService methods are stubs

**Location**: `marketplace.rs:104-110`

**Problem**: `list_official_plugins()` and `list_repository_plugins()` return empty vectors with TODO comments - not actually implemented.

```rust
pub fn list_official_plugins() -> Vec<MarketplacePlugin> {
    Vec::new()  // TODO
}

pub fn list_repository_plugins() -> Vec<MarketplacePlugin> {
    Vec::new()  // TODO
}
```

This is documented but not implemented - the architecture should note these are TODO.

---

### BUG 4 (LOW): dispatch_to_plugin referenced but doesn't exist

**Location**: `architecture/plugin.md:35` mentions `PluginEventBus` but this is not a bug - the dead `dispatch_to_plugin` function mentioned in AGENTS.md was already removed.

---

### DISCREPANCY (MINOR): api.rs duplicates hooks types

**Location**: `api.rs:39-118`

**Problem**: `api::hooks` module re-defines `HookType`, `HookContext`, `HookResult` which are identical to those in `hooks.rs`. This duplication could lead to inconsistency.

The architecture doc shows these as separate but doesn't mention they are duplicates. The `api.rs` types are for external/plugin-facing API while `hooks.rs` are internal.

---

### DISCREPANCY (MINOR): PluginService has additional undocumented method

**Location**: `service.rs:27-29`

**Problem**: `registry()` accessor method not documented in architecture.

```rust
pub fn registry(&self) -> &Arc<PluginRegistry> {
    &self.registry
}
```

---

## Improvement Suggestions

### Priority: HIGH

1. **Fix WASM path construction in `execute_wasm_hook`**
   - The bug means WASM plugins cannot be loaded correctly
   - Need to strip `plugin:` prefix or use bare plugin name for path
   - Impact: All WASM plugins fail to execute

2. **Export `BuiltinPlugin` struct** or document it as internal-only
   - Currently unused externally but part of public module

### Priority: MEDIUM

3. **Document MarketplaceService stubs as TODO**
   - Update architecture to note `list_official_plugins()` and `list_repository_plugins()` are not implemented
   - Or implement them

4. **Add `dispatch_provider()` to architecture doc**
   - The method exists in `service.rs:239-245` but isn't documented
   - Only missing from individual dispatch methods list

### Priority: LOW

5. **Consider consolidating duplicate hook types**
   - `api::hooks` duplicates `hooks.rs` types
   - Could have `api::hooks` re-export from `hooks` module
   - Lower priority - may be intentional for API stability boundary

6. **Document `PluginService::registry()` accessor**
   - Add to architecture or remove if not part of public API

7. **Update skill doc to match current implementation**
   - SKILL.md version is 1.1.0 but may need updates after recent fixes

---

## Summary

| Category | Count |
|----------|-------|
| Verified Claims | 35+ |
| Bugs Found | 2 |
| Discrepancies | 3 |
| Improvement Suggestions | 7 |

**Overall Assessment**: The architecture documentation is highly accurate and well-maintained. Most types, methods, and behaviors match the implementation exactly. The main bug is the WASM path construction issue which would prevent WASM plugins from loading correctly. The skill documentation is also largely accurate and synchronized with the implementation.
# Plugin Module Architecture Review

**Date:** 2026-05-24  
**Reviewer:** Architecture Review Agent  
**Module:** `plugin`  
**Files Reviewed:**  
- `architecture/plugin.md`  
- `src/plugin/` (mod.rs, hooks.rs, loader.rs, service.rs, registry.rs, manifest.rs, install.rs, tui.rs, event_bus.rs, marketplace.rs, api.rs, builtin/mod.rs, builtin/*.rs)  
- `.opencode/skills/plugin/SKILL.md`

---

## Summary of Verification

| Item | Status | Notes |
|------|--------|-------|
| Project Structure | VERIFIED | All files match architecture doc |
| HookType enum (12 variants) | VERIFIED | All present and correct |
| HookType::as_str() dot notation | VERIFIED | Returns correct values |
| HookContext/HookResult | VERIFIED | Match exactly |
| PluginService dispatch methods | VERIFIED | All 12 dispatch methods present |
| PluginRegistry methods | VERIFIED | register, hooks_for, is_enabled, set_enabled all correct |
| Fuel tracking constants | VERIFIED | All constants match (MAX_WASM_SIZE, WASM_FUEL_PER_HOOK, etc.) |
| WASM execution path | VERIFIED | execute_wasm_hook implemented correctly |
| install.rs functions | VERIFIED | All present and correct |
| Builtin plugins (copilot, gitlab, codex, poe) | VERIFIED | All registered with auth hooks |
| MarketplaceService | VERIFIED | list_local_plugins, search_plugins present |
| TuiPluginRegistry | VERIFIED | All fields match |
| Feature flag | VERIFIED | Uses `plugins` not `plugin` |
| PluginEventBus | VERIFIED | Structure correct |

---

## Discrepancies Found

### 1. WASM Path Building (Minor - Doc Error)

**Location:** `architecture/plugin.md:178-179` and `src/plugin/loader.rs:276-277`

**Issue:** The architecture doc shows:
```rust
let wasm_path: PathBuf = format!("plugins/{}/plugin.wasm", plugin_id).into();
```

But actual code strips the `plugin:` prefix:
```rust
let plugin_name = plugin_id.strip_prefix("plugin:").unwrap_or(plugin_id);
let wasm_path = crate::plugin::install::plugins_dir().join(plugin_name).join("plugin.wasm");
```

This effectively produces the same result (looking in `~/.local/share/codegg/plugins/{name}/plugin.wasm`), but the doc should reflect the actual implementation path construction.

**Recommendation:** Update architecture doc to show the actual WASM path construction that uses `plugins_dir()`.

---

### 2. BUILTIN_HANDLERS Type Declaration (Doc Inaccuracy)

**Location:** `architecture/plugin.md:272-281` and `src/plugin/builtin/mod.rs:18-39`

**Issue:** The architecture doc shows:
```rust
static BUILTIN_HANDLERS: std::sync::LazyLock<
    RwLock<HashMap<String, fn(HookContext) -> HookResult>>,
> = std::sync::LazyLock::new(|| {
```

But actual code uses explicit type casting in the insert:
```rust
handlers.insert(
    "copilot".to_string(),
    copilot::handle_hook as fn(HookContext) -> HookResult,
);
```

**Impact:** None - functionality is identical. This is purely a documentation style difference.

**Recommendation:** Update doc to show the `as fn(HookContext) -> HookResult` casting style used in actual code.

---

### 3. `dispatch_to_plugin` Function (Dead Code Removed)

**Location:** `architecture/plugin.md:35` mentions `event_bus.rs` contains `PluginEventBus, PluginEventSubscription`

**Issue:** The AGENTS.md notes indicate `dispatch_to_plugin` was removed from `event_bus.rs` (was at lines 63-69), but `architecture/plugin.md` still references the function implicitly through the hook flow diagram and text.

The architecture doc does NOT explicitly document `dispatch_to_plugin` function signature, but mentions "PluginEventBus" and "PluginEventSubscription". These exist correctly. The issue is only that the flow diagram references "dispatch_to_plugin" implicitly.

**Verification:** `src/plugin/event_bus.rs` contains no `dispatch_to_plugin` function - it was correctly removed as noted in AGENTS.md.

**Status:** Already fixed per AGENTS.md notes - architecture doc may still reference it in flow descriptions.

---

### 4. Timeout Error Message Format (Minor)

**Location:** `src/plugin/service.rs:108`

**Issue:** The skill doc says timeout errors use format `"{plugin_id}: hook timeout: {err}"` but the actual error message is:
```rust
return HookResult::error(format!("{}: hook timeout: {}", hook.plugin_id, err));
```

**Impact:** Very minor - format string uses positional `{}` rather than named `{err}`, but output is identical.

**Recommendation:** Update skill doc to show `format!("{}: hook timeout: {}", hook.plugin_id, err)` or standardize the code.

---

## Potential Bugs / Issues in Code

### 1. `check_and_reset_fuel_budget()` Never Called

**Location:** `src/plugin/loader.rs:24-41`

The function `check_and_reset_fuel_budget()` exists and appears designed to reset the global fuel budget every 60 seconds, but it is never called anywhere in the codebase.

**Code:**
```rust
#[allow(dead_code)]
fn check_and_reset_fuel_budget() {
    // ... implementation that would reset PLUGIN_FUEL_BUDGET
}
```

**Impact:** The fuel budget auto-reset documented in `architecture/plugin.md:384-386` will not occur. Per-plugin fuel tracking via `reserve_fuel()` and `return_fuel()` still works, but the global budget reset mechanism is dead code.

**Recommendation:** Either:
1. Call `check_and_reset_fuel_budget()` somewhere in the plugin execution path
2. Remove the function if not needed
3. Update docs to clarify global budget reset is not implemented

---

### 2. `PLUGIN_FUEL_BUDGET` Global Never Actually Used

**Location:** `src/plugin/loader.rs:15`

**Code:**
```rust
static PLUGIN_FUEL_BUDGET: AtomicU64 = AtomicU64::new(10_000_000);
static PLUGIN_FUEL_LAST_RESET: AtomicU64 = AtomicU64::new(0);
```

**Issue:** The global `PLUGIN_FUEL_BUDGET` is defined but `check_and_reset_fuel_budget()` never runs. The per-plugin fuel budgets in `ModuleCache::fuel_budgets` work correctly via `get_plugin_fuel()`, `reserve_fuel()`, and `return_fuel()`.

**Impact:** The global budget exists but has no effect. The per-plugin budgets function correctly.

**Recommendation:** Document that per-plugin fuel budgets are the active mechanism.

---

### 3. Unused Fields in PluginEventBus

**Location:** `src/plugin/event_bus.rs:14-18`

**Code:**
```rust
pub struct PluginEventBus {
    subscriptions: Arc<RwLock<Vec<PluginEventSubscription>>>,
    event_log: Arc<RwLock<Vec<AppEvent>>>,  // Only written, never read by anything
    max_log_size: usize,
}
```

**Issue:** `event_log` is populated in `publish()` but never read by any consumer. The `get_event_log()` method exists but is never called from outside the module.

**Impact:** Low - code is clean, just unused functionality.

**Recommendation:** Either use `event_log` somewhere or remove the `get_event_log()` method if it's not needed.

---

## Documentation Issues

### 1. Architecture doc line 178-179 shows wrong WASM path construction

Should show:
```rust
let plugin_name = plugin_id.strip_prefix("plugin:").unwrap_or(plugin_id);
let wasm_path = crate::plugin::install::plugins_dir().join(plugin_name).join("plugin.wasm");
```

### 2. Architecture doc doesn't mention `BuiltinPlugin` struct in `builtin/mod.rs`

The struct is defined at `src/plugin/builtin/mod.rs:13-16` but not documented in the architecture.

### 3. Skill doc shows incorrect `execute_wasm_hook()` implementation

The skill doc shows `std::fs::metadata` and `std::fs::read` but actual code at `loader.rs:280-301` shows proper error handling with `match` statements.

---

## Verified Correct Items (No Action Needed)

1. **HookType enum** - All 12 variants with correct dot notation serialization
2. **HookResult::ok(), blocked(), error()** - All implemented correctly
3. **PluginService** - All 12 dispatch methods present, default 5s timeout
4. **PluginRegistry::hooks_for()** - Returns Vec<HookRegistration> correctly filtered
5. **registry.rs:sort_hooks()** - Sorts by priority at registration time
6. **loader.rs:189-226** - Per-plugin fuel tracking via ModuleCache works correctly
7. **install.rs:128-162** - extract_plugin_archive() validates paths correctly
8. **install.rs:179-213** - copy_dir_all() validates symlinks and paths
9. **Builtin plugins auth handling** - All four handle only their specific provider
10. **Feature flag** - Correctly uses `plugins` feature, not `plugin`

---

## Recommendations

### High Priority

1. **Fix `check_and_reset_fuel_budget()`** - Either call it in the plugin execution path or remove dead code
2. **Update WASM path construction in architecture doc** - Show actual implementation using `plugins_dir()`

### Medium Priority

3. **Document `BuiltinPlugin` struct** - Add to architecture doc
4. **Update skill doc `execute_wasm_hook()`** - Show actual error handling flow

### Low Priority

5. **Consider removing unused `event_log`** - Or find a use for it
6. **Standardize timeout error format** - Pick one style and use consistently

---

## File:Line References for Issues

| Issue | File | Lines |
|-------|------|-------|
| check_and_reset_fuel_budget not called | `src/plugin/loader.rs` | 24-41, never called |
| PLUGIN_FUEL_BUDGET unused | `src/plugin/loader.rs` | 15 |
| event_log never read | `src/plugin/event_bus.rs` | 16, 67 |
| WASM path doc inaccuracy | `architecture/plugin.md` | 178-179 |
| BuiltinPlugin not documented | `architecture/plugin.md` | - |
| Skill doc execute_wasm_hook example | `.opencode/skills/plugin/SKILL.md` | 183-190 |
| Timeout error format | `src/plugin/service.rs` | 108 |

---

## Conclusion

The plugin module implementation is **largely correct** and well-architected. The main discrepancies are:

1. **Documentation issues** - WASM path construction shown incorrectly, `BuiltinPlugin` struct undocumented
2. **Dead code** - `check_and_reset_fuel_budget()` and `PLUGIN_FUEL_BUDGET` global are unused
3. **Minor formatting differences** - Error message format and type declarations differ slightly from docs

The core functionality (WASM loading, hook dispatch, fuel tracking, builtin plugins, installation) is **correctly implemented** and matches the documented behavior.

**Overall Assessment:** The implementation is sound. Fix the documentation issues and consider removing or utilizing the dead code.

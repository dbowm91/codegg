# Plugin Module Architecture Review

**Reviewed**: `architecture/plugin.md` vs `src/plugin/` source code  
**Date**: 2026-05-26  
**Result**: ✅ MOSTLY ACCURATE - Minor discrepancies and omissions noted

---

## Summary

The architecture document is largely accurate and well-structured. All major types, structures, and behaviors described in the documentation match the actual implementation. There are a few minor discrepancies and undocumented elements.

---

## 1. Project Structure ✅

**Doc Claim** (lines 25-43): Lists 11 files + builtin/ submodule

**Actual** (`src/plugin/`):
- mod.rs ✅
- loader.rs ✅
- hooks.rs ✅
- registry.rs ✅
- manifest.rs ✅
- service.rs ✅
- install.rs ✅
- api.rs ✅
- tui.rs ✅
- event_bus.rs ✅ (not mentioned in doc!)
- marketplace.rs ✅
- builtin/mod.rs ✅
- builtin/copilot.rs ✅
- builtin/gitlab.rs ✅
- builtin/codex.rs ✅
- builtin/poe.rs ✅

**Finding**: `event_bus.rs` is not documented but exists. All other files match.

---

## 2. Key Types

### HookType ✅

**Doc Claim** (lines 85-101): 13 variants with snake_case serialization

**Actual** (`src/plugin/hooks.rs:6-20`):
```rust
pub enum HookType {
    Auth,              // "auth"
    Provider,          // "provider"
    ToolDefinition,    // "tool.definition"
    ToolExecuteBefore, // "tool.execute.before"
    ToolExecuteAfter,  // "tool.execute.after"
    ChatParams,        // "chat.params"
    ChatHeaders,       // "chat.headers"
    Event,             // "event"
    Config,            // "config"
    ShellEnv,          // "shell.env"
    TextComplete,      // "text.complete"
    SessionCompacting, // "session.compacting"
    MessagesTransform, // "messages.transform"
}
```

**Finding**: Matches exactly. All 13 variants present with correct dot-notation mappings via `as_str()` (hooks.rs:23-38) and `parse()` (hooks.rs:41-58).

### HookContext ✅

**Doc Claim** (lines 106-111):
```rust
pub struct HookContext {
    pub hook_type: HookType,
    pub input: serde_json::Value,
}
```

**Actual** (`src/plugin/hooks.rs:62-65`): Exact match.

### HookResult ✅

**Doc Claim** (lines 115-127): `blocked: bool` field

**Actual** (`src/plugin/hooks.rs:68-71`):
```rust
pub struct HookResult {
    pub output: serde_json::Value,
    pub blocked: bool,
    pub error: Option<String>,
}
```

**Finding**: Matches. Methods `ok()`, `blocked()`, `error()` present at hooks.rs:74-97.

### HookRegistration ✅

**Doc Claim** (lines 172-176):
```rust
pub struct HookRegistration {
    pub plugin_id: String,
    pub hook_type: HookType,
    pub priority: i32,
}
```

**Actual** (`src/plugin/hooks.rs:100-105`): Exact match.

### PluginManifest ✅

**Doc Claim** (lines 49-61):
```rust
pub struct PluginManifest {
    pub name: String,
    pub version: String,
    pub description: Option<String>,
    pub author: Option<String>,
    pub homepage: Option<String>,
    pub license: Option<String>,
    #[serde(default)]
    pub hooks: Vec<HookSpec>,
    #[serde(default)]
    pub config: HashMap<String, serde_json::Value>,
}
```

**Actual** (`src/plugin/manifest.rs:4-16`): Exact match.

### HookSpec ✅

**Doc Claim** (lines 63-67):
```rust
pub struct HookSpec {
    #[serde(rename = "type")]
    pub hook_type: String,  // dot notation, e.g., "tool.execute.before"
    pub priority: Option<i32>,
}
```

**Actual** (`src/plugin/manifest.rs:18-23`): Exact match.

### LoadedPlugin ✅

**Doc Claim** (lines 72-78):
```rust
pub struct LoadedPlugin {
    pub manifest: PluginManifest,
    pub wasm_path: PathBuf,
    pub plugin_dir: PathBuf,
}
```

**Actual** (`src/plugin/loader.rs:32-36`): Exact match.

---

## 3. Components

### loader.rs ✅

**Doc Claim** (lines 131-157): ModuleCache structure with `modules`, `hits`, `misses`, `fuel_budgets`

**Actual** (`src/plugin/loader.rs:109-114`):
```rust
pub struct ModuleCache {
    modules: DashMap<String, (Module, u64)>,
    hits: AtomicU64,
    misses: AtomicU64,
    fuel_budgets: DashMap<String, AtomicU64>,
}
```

**Finding**: Matches. Methods `get_or_compile()`, `get_plugin_fuel()`, `reserve_fuel()`, `return_fuel()` present at lines 126-208.

### Constants ✅

**Doc Claim** (lines 389-393):
```rust
const MAX_WASM_SIZE: usize = 10 * 1024 * 1024;  // 10MB
const WASM_FUEL_PER_HOOK: u64 = 1_000_000;
const WASM_HOOK_TIMEOUT: Duration = Duration::from_secs(30);
const MAX_PLUGIN_FUEL_BUDGET: u64 = 10_000_000;
```

**Actual** (`src/plugin/loader.rs:8-15`): Exact match (with `#[allow(dead_code)]`).

### PluginService ⚠️

**Doc Claim** (lines 188-207): 
- `hook_timeout: Duration` (default 5 seconds) ✅
- `dispatch_auth()`, `dispatch_provider()`, `dispatch_tool_execute_before()`, `dispatch_tool_execute_after()` etc. ✅

**Actual** (`src/plugin/service.rs:9-12`):
```rust
pub struct PluginService {
    registry: Arc<PluginRegistry>,
    hook_timeout: Duration,
}
```

**Finding**: 
1. All documented dispatch methods exist (service.rs:143-245)
2. `dispatch_provider()` is present (service.rs:239-245) - not explicitly listed in doc but implied
3. Undocumented method: `registry()` accessor at service.rs:27-29

### registry.rs ⚠️

**Doc Claim** (lines 210-233): Methods `register`, `unregister`, `hooks_for`, `is_enabled`, `set_enabled`

**Actual** (`src/plugin/registry.rs:22-88`):
- `register()` ✅
- `unregister()` ✅
- `hooks_for()` ✅
- `is_enabled()` ✅
- `set_enabled()` ✅
- **Additional undocumented methods**:
  - `new()` (registry.rs:23)
  - `get()` (registry.rs:42)
  - `list()` (registry.rs:46)
  - `enabled_plugins()` (registry.rs:50)

### tui.rs ✅

**Doc Claim** (lines 245-266):
```rust
pub struct TuiPluginRegistry {
    routes: Arc<RwLock<Vec<TuiRoute>>>,
    components: Arc<RwLock<Vec<TuiComponent>>>,
    plugin_configs: Arc<RwLock<HashMap<String, serde_json::Value>>>,
}

pub struct TuiRoute {
    pub path: String,
    pub label: String,
    pub plugin_id: String,
    pub icon: Option<String>,
}

pub struct TuiComponent {
    pub name: String,
    pub plugin_id: String,
    pub config: serde_json::Value,
}
```

**Actual** (`src/plugin/tui.rs:6-25`): Exact match.

### marketplace.rs ⚠️

**Doc Claim** (lines 300-313): `MarketplaceService` with `list_local_plugins`, `search_plugins`, `list_official_plugins`, `list_repository_plugins`

**Actual** (`src/plugin/marketplace.rs`): 
- All 4 methods present ✅
- `list_official_plugins()` and `list_repository_plugins()` return `Vec::new()` (empty) - not actual TODO implementations ❌

**Finding**: Doc says "TODO" for official/repository but they are implemented (returning empty Vec).

**Additional undocumented field**: `MarketplacePlugin` has `id` and `tier` fields (marketplace.rs:22-31) not shown in doc.

### install.rs ✅

**Doc Claim** (lines 235-243):
```rust
pub fn plugins_dir() -> PathBuf;  // ~/.local/share/codegg/plugins
pub async fn install_from_path(path: &Path) -> Result<PathBuf, InstallError>;
pub async fn install_from_url(url: &str) -> Result<PathBuf, InstallError>;
pub async fn uninstall(plugin_name: &str) -> Result<(), InstallError>;
```

**Actual** (`src/plugin/install.rs`):
- `plugins_dir()` ✅ (install.rs:23-28)
- `install_from_path()` ✅ (install.rs:30-68)
- `install_from_url()` ✅ (install.rs:70-126)
- `uninstall()` ✅ (install.rs:164-177)

**Additional undocumented helper**: `copy_dir_all()` (install.rs:179-214)

### builtin/mod.rs ✅

**Doc Claim** (lines 268-296):
- `BUILTIN_HANDLERS` static ✅
- `builtin_hook_handler()` ✅
- `register_builtins()` ✅
- `BuiltinPlugin` struct ✅

**Actual** (`src/plugin/builtin/mod.rs`): All match.

**Finding**: Built-in plugins (copilot, gitlab, codex, poe) all provide `auth` hook handlers that inject Bearer tokens - verified in each builtin module.

---

## 4. Security Section ✅

**Doc Claim** (lines 404-412):
- Fuel Limits ✅
- Timeout (5s outer, 30s inner) ✅
- Memory Bounds ✅
- Output Size (10MB) ✅
- WASM Size (10MB) ✅
- Path Traversal validation ✅
- Symlinks not allowed ✅

**Actual verification**:
- Fuel tracking: loader.rs:178-199 (reserve_fuel), 202-208 (return_fuel)
- Outer timeout: service.rs:121 (`self.hook_timeout` default 5s from service.rs:18)
- Inner timeout: loader.rs:291 (`WASM_HOOK_TIMEOUT` 30s)
- Memory bounds: loader.rs:384-391
- Output size: loader.rs:424-434
- WASM size: loader.rs:263-272
- Path traversal: install.rs:136-161
- Symlinks: install.rs:142-146, 191-195

---

## 5. Feature Flag ✅

**Doc Claim** (lines 416-421):
```toml
plugins = ["dep:wasmtime", "dep:wasmtime-cache", "dep:wasmtime-wasi"]
```

**Actual** (`Cargo.toml:171`):
```toml
plugins = ["wasmtime", "wasmtime-wasi"]
```

**Finding**: Minor discrepancy - `wasmtime-cache` is not in actual Cargo.toml (may have been removed or doc is forward-looking).

---

## 6. Plugin IDs ✅

**Doc Claim** (lines 424-426):
- WASM plugins: `plugin:{name}`
- Built-in plugins: `builtin:{name}`

**Actual**: Verified in `service.rs:32` (`format!("plugin:{}", ...)`) and `builtin/mod.rs:53` (`format!("builtin:{}", ...)`).

---

## 7. Fuel Tracking ✅

**Doc Claim** (lines 384-403): All fuel logic statements

**Actual verification**:
- Constants at loader.rs:8-15 ✅
- Reserve before execution: loader.rs:244-248 ✅
- Store set_fuel: loader.rs:293 ✅
- Return on errors: loader.rs:258, 270, 286, 329, 338, 353, 371, 386, 500, 505, 510 ✅
- Budget exhausted returns early: loader.rs:236-240 ✅

**Finding**: All fuel paths correctly call `return_fuel()` including error paths (lines 258, 270, 286, etc.).

---

## 8. Hook Flow Diagram ✅

**Doc Claim** (lines 349-382): Flow diagram

**Actual**: Verified against `service.rs:63-114` (dispatch_hook) and `service.rs:116-141` (execute_hook_with_timeout). Flow is accurate.

---

## 9. Undocumented Elements

| Element | Location | Description |
|---------|----------|-------------|
| `event_bus.rs` | `src/plugin/event_bus.rs:1-76` | PluginEventBus, PluginEventSubscription not in doc |
| `PluginEventBus` | event_bus.rs:14-18 | Publish/subscribe for plugin events |
| `PluginEventSubscription` | event_bus.rs:7-12 | Subscription with event patterns |
| `registry()` accessor | service.rs:27-29 | Returns &Arc<PluginRegistry> |
| `PluginRegistry::get()` | registry.rs:42 | Get plugin by ID |
| `PluginRegistry::list()` | registry.rs:46 | List all plugins |
| `PluginRegistry::enabled_plugins()` | registry.rs:50 | List enabled plugins |
| `copy_dir_all()` | install.rs:179-214 | Helper for installing from path |
| `MarketplacePlugin.tier` | marketplace.rs:29 | PluginTier enum field |
| `MarketplacePlugin.id` | marketplace.rs:24 | Additional ID field |
| `ApiVersion` module | api.rs:18-37 | API version info not in plugin.md |

---

## 10. Minor Discrepancies

| Issue | Doc Line | Actual | Impact |
|-------|----------|--------|--------|
| Feature flag | 420 | `plugins = ["wasmtime", "wasmtime-wasi"]` | Missing `wasmtime-cache` |
| MarketplaceService | 310-311 | Returns `Vec::new()` | Says TODO but implemented (empty) |
| MarketplacePlugin fields | 302-305 | Missing `id`, `tier` | Incomplete struct definition |
| PluginRegistry | 226-232 | Has `get`, `list`, `enabled_plugins` | Additional methods not documented |

---

## Conclusion

The architecture document is **largely accurate** with minor discrepancies:

1. **Critical issues**: None
2. **Missing elements**: `event_bus.rs`, additional PluginRegistry methods, `MarketplacePlugin.id/tier`
3. **Minor**: Feature flag missing `wasmtime-cache`, doc says TODO for marketplace methods but they're implemented (returning empty Vec)

The document serves as a good overview but should be updated to include the undocumented `event_bus.rs` module and the additional methods in `PluginRegistry`.
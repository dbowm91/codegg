# Plugin Module

The `plugin` module provides a WASM-based plugin system for extending agent capabilities.

## Overview

**Location**: `src/plugin/`

**Key Responsibilities**:
- WASM plugin loading and execution
- Plugin manifest parsing
- Hook system for plugins
- TUI extensions
- Plugin installation and registry

## Technology

Uses **Wasmtime** runtime for WASM execution (feature-gated with `plugin` flag).

## Key Types

### PluginManifest

```rust
pub struct PluginManifest {
    pub name: String,
    pub version: String,
    pub description: String,
    pub author: String,
    pub hooks: Vec<HookDefinition>,
    pub tools: Vec<ToolDefinition>,
    pub permissions: Vec<String>,
}
```

### LoadedPlugin

```rust
pub struct LoadedPlugin {
    pub manifest: PluginManifest,
    pub instance: wasmtime::Instance,
    pub exports: wasmtime::Exports,
}
```

## Components

### loader.rs - WASM Loading

```rust
pub struct PluginLoader {
    engine: Arc<wasmtime::Engine>,
}

impl PluginLoader {
    pub fn load(&self, wasm_bytes: &[u8]) -> Result<LoadedPlugin>;
    pub fn validate(&self, wasm_bytes: &[u8]) -> Result<Manifest>;
}
```

### manifest.rs - Manifest Parsing

Parses plugin metadata from `plugin.json`:

```rust
pub fn parse_manifest(json: &str) -> Result<PluginManifest>;
```

### hooks.rs - Hook System

Plugins can register hooks for lifecycle events:

```rust
pub struct HookDefinition {
    pub name: String,
    pub event: HookEvent,
}

pub enum HookEvent {
    PreToolExecute,
    PostToolExecute,
    PreAgentRun,
    PostAgentRun,
    SessionStart,
    SessionEnd,
}
```

**HookContext** - Data passed to hooks:
```rust
pub struct HookContext {
    pub event: HookEvent,
    pub data: Value,
}
```

**HookResult** - Return from hooks:
```rust
pub enum HookResult {
    Continue,
    Stop,
    Modify(Value),
}
```

### service.rs - Plugin Service

```rust
pub struct PluginService {
    registry: PluginRegistry,
    loader: PluginLoader,
    hook_registry: HookRegistry,
}

impl PluginService {
    pub fn install(&self, path: &Path) -> Result<String>;
    pub fn uninstall(&self, name: &str) -> Result<()>;
    pub fn list_plugins(&self) -> Vec<PluginManifest>;
    pub async fn run_hooks(&self, event: HookEvent, ctx: HookContext) -> HookResult;
}
```

### registry.rs - Plugin Registry

```rust
pub struct PluginRegistry {
    plugins: RwLock<HashMap<String, LoadedPlugin>>,
}
```

### tui.rs - TUI Extensions

Allows plugins to add custom UI:

```rust
pub trait TuiExtension: Send + Sync {
    fn render(&self, area: Rect, buf: &mut Buffer);
    fn handle_key(&self, key: Key) -> bool;
}
```

### install.rs - Plugin Installation

```rust
pub struct PluginInstaller {
    store: PathBuf,
}

impl PluginInstaller {
    pub async fn install_from_url(&self, url: &str) -> Result<String>;
    pub async fn install_from_file(&self, path: &Path) -> Result<String>;
}
```

## Plugin Directory Structure

```
~/.config/codegg/plugins/
├── my-plugin/
│   ├── plugin.wasm
│   ├── plugin.json
│   └── hooks/
│       └── pre_tool_execute.js
```

## Hook Flow

```
AgentLoop
├── PreToolExecute hook
│   └── PluginService::run_hooks(HookEvent::PreToolExecute, ctx)
│       └── Returns HookResult::Continue | Stop | Modify
│
├── Tool execution
│
└── PostToolExecute hook
    └── PluginService::run_hooks(HookEvent::PostToolExecute, ctx)
```

## Fuel Tracking

Plugins use fuel to limit resource consumption:

```rust
pub struct FuelPolicy {
    max_fuel: u64,
    fuel_per_tick: u64,
}

pub struct FuelTracker {
    remaining: u64,
}
```

## Security

- Plugins run in WASM sandbox
- Fuel tracking prevents infinite loops
- Explicit permissions required in manifest
- Hook results validated before execution

## Feature Flag

Requires `plugin` feature in `Cargo.toml`:

```toml
[features]
plugin = ["dep:wasmtime"]
```

## See Also

- [hooks.md](hooks.md) - Hook system details
- [agent.md](agent.md) - AgentLoop integration
- [tool.md](tool.md) - Tool execution

# Phase 11 Plan: Plugin Management UX

## Objective

Add first-class plugin management commands and UI surfaces so users can inspect, enable, disable, diagnose, and remove plugins from Codegg without reading manifests or internal logs.

This phase builds on the runtime/capability registry, frontend-neutral `UiNode` renderer, and corrective hardening already in place. It should not introduce a plugin marketplace or remote install flow. The goal is local observability and controlled management.

## Command Set

Implement these slash commands:

- `/plugins`
- `/plugin-info <plugin-id-or-name>`
- `/plugin-enable <plugin-id-or-name>`
- `/plugin-disable <plugin-id-or-name>`
- `/plugin-doctor [plugin-id-or-name]`
- `/plugin-remove <plugin-id-or-name>`
- `/plugin-install <path-or-url>` as local-path only in this phase unless URL install already exists safely

Prefer aliases only where obvious and low conflict:

- `/plugin-list` -> `/plugins`
- `/plugin-ls` -> `/plugins`

Do not overload existing command names.

## UI Principle

All plugin management output should use the portable UI schema:

- tables for plugin lists;
- key-value nodes for plugin info;
- containers for grouped diagnostics;
- markdown/text nodes for explanatory notes;
- toasts for short success/failure feedback;
- dialogs for detailed inspection.

Avoid building a new ratatui-only plugin management component unless necessary. Management UI should exercise the shared `UiNode` path.

## Files to Add

### `src/tui/commands/plugin_management.rs`

Add TUI command handlers:

```rust
pub(crate) fn show_plugins(app: &mut App);
pub(crate) fn show_plugin_info(app: &mut App, query: &str);
pub(crate) fn enable_plugin(app: &mut App, query: &str);
pub(crate) fn disable_plugin(app: &mut App, query: &str);
pub(crate) fn doctor_plugin(app: &mut App, query: Option<&str>);
pub(crate) fn remove_plugin(app: &mut App, query: &str);
pub(crate) fn install_plugin(app: &mut App, query: &str);
```

Keep command parsing thin. Registry/service work should live in plugin modules.

### `src/plugin/management.rs`

Add backend/service helpers usable by TUI and future daemon clients:

```rust
pub struct PluginManager {
    service: Arc<PluginService>,
}

impl PluginManager {
    pub async fn list(&self) -> Vec<PluginManagementView>;
    pub async fn info(&self, selector: &str) -> Result<PluginManagementView, PluginManagementError>;
    pub async fn enable(&self, selector: &str) -> Result<PluginManagementView, PluginManagementError>;
    pub async fn disable(&self, selector: &str) -> Result<PluginManagementView, PluginManagementError>;
    pub async fn doctor(&self, selector: Option<&str>) -> PluginDoctorReport;
    pub async fn remove(&self, selector: &str) -> Result<PluginManagementView, PluginManagementError>;
}
```

If a full `PluginManager` is too heavy, add equivalent functions under `PluginService` but avoid mixing UI formatting into registry code.

### `src/plugin/management_ui.rs`

Add helpers that convert management views into `UiNode`:

```rust
pub fn plugins_table(plugins: &[PluginManagementView]) -> UiNode;
pub fn plugin_info_node(plugin: &PluginManagementView) -> UiNode;
pub fn doctor_report_node(report: &PluginDoctorReport) -> UiNode;
```

These helpers should live outside TUI so other frontends can reuse them.

## Files to Modify

### `src/tui/command.rs`

Register the new commands and descriptions in the command registry.

Ensure completions display concise descriptions:

- `/plugins` — List installed and built-in plugins
- `/plugin-info` — Show plugin runtime, capabilities, trust, and diagnostics
- `/plugin-enable` — Enable a plugin
- `/plugin-disable` — Disable a plugin
- `/plugin-doctor` — Diagnose plugin configuration and runtime health
- `/plugin-remove` — Remove a local installed plugin
- `/plugin-install` — Install a plugin from a local path

### `src/tui/runtime/command_dispatch.rs` or slash command execution path

Route plugin management commands into `src/tui/commands/plugin_management.rs`.

Use existing async spawn-and-complete patterns for commands that acquire locks, read files, or mutate plugin state.

### `src/plugin/registry.rs`

Add selector helpers if not already present:

```rust
pub async fn resolve_plugin_selector(&self, selector: &str) -> Result<PluginInfo, PluginRegistryError>;
```

Resolution order:

1. exact plugin id;
2. exact manifest name;
3. unique prefix match on id;
4. unique prefix match on name;
5. error on none or ambiguous.

### `src/plugin/install.rs`

Expose safe local install/remove operations if already implemented. If installer semantics are not mature, restrict this phase to validation plus explicit warning.

Avoid URL/network installs unless there is already a hardened fetcher. Local path install is enough.

## Management View Fields

`PluginManagementView` should include:

- plugin id;
- name;
- version;
- api version;
- enabled state;
- runtime kind;
- trust class;
- install/source path;
- command count;
- hook count;
- panel count;
- status widget count;
- event subscription count;
- permission summary;
- diagnostics count;
- last error if tracked.

For `/plugins`, table columns should be compact:

```text
ID | Name | Version | Runtime | Trust | Enabled | Capabilities | Diagnostics
```

For `/plugin-info`, show full details grouped by section.

## Doctor Checks

`/plugin-doctor` should report:

- manifest parse validity;
- API version compatibility;
- runtime availability;
- WASM feature enabled/disabled state;
- process command existence/path resolution;
- plugin enable state;
- duplicate capability conflicts;
- permission/trust warnings;
- declared output surfaces;
- stale/inaccessible install path;
- last runtime error if tracked;
- registry index consistency.

Do not execute arbitrary plugin code during doctor by default. A separate future `--run-smoke` option can be considered later.

## Safety Semantics

### Enable/Disable

Enable/disable should mutate registry state and persist if there is already a plugin config/prefs file. If persistence does not exist, implement runtime-only toggling and document it clearly in the UI.

Do not silently enable duplicate commands. The registry should reject conflicts deterministically.

### Remove

Remove should:

1. disable plugin;
2. unregister from registry;
3. remove installed files only if the plugin is under Codegg’s plugin install directory;
4. refuse to delete arbitrary project paths by default;
5. show exact path that was removed.

### Install

Local path install should:

- validate manifest before copying;
- reject path traversal;
- reject missing runtime artifacts;
- refuse to overwrite an existing plugin unless explicit flag/support exists;
- show capability and trust summary before enabling, or install disabled by default.

If interactive confirmation is not easy yet, install disabled by default.

## Tests

Add unit tests for management helpers:

- selector resolves exact id;
- selector resolves exact name;
- ambiguous prefix errors;
- plugin table includes runtime/trust/enabled fields;
- plugin info includes permissions and capabilities;
- doctor reports missing process executable;
- doctor reports WASM runtime disabled when feature is off;
- remove refuses paths outside plugin install dir;
- install rejects invalid manifest;
- enable rejects duplicate command conflict.

Add TUI command tests:

- `/plugins` opens a dialog/table;
- `/plugin-info missing` shows useful error;
- `/plugin-disable builtin:codex` toggles state or reports not persistent;
- `/plugin-enable` surfaces duplicate error;
- `/plugin-doctor` renders diagnostics via `UiNode`.

## Documentation Updates

Update:

- `docs/PLUGINS.md` with management commands;
- `architecture/plugin.md` with management flow;
- `.opencode/skills/plugin/SKILL.md` with command names and safety rules.

Include clear statements:

- process plugins are local executables;
- WASM plugins are sandboxed by Wasmtime limits, not fully untrusted arbitrary code magic;
- builtins are first-party trusted code;
- enable/disable persistence behavior.

## Acceptance Criteria

- `/plugins` lists registered plugins with runtime/trust/enabled/capability summary.
- `/plugin-info` renders detailed plugin info through `UiNode`.
- `/plugin-enable` and `/plugin-disable` work and surface registry errors.
- `/plugin-doctor` reports actionable diagnostics without executing plugin code by default.
- `/plugin-remove` is safe and refuses arbitrary path deletion.
- `/plugin-install` supports local paths or explicitly reports unsupported install mode.
- Management output uses portable UI nodes rather than bespoke ratatui-only views.
- Tests cover selector resolution, doctor checks, enable/disable, install/remove safety, and TUI rendering.

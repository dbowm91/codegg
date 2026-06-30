# Phase 2 Implementation Plan: TUI Renderer Adapter

## Summary

Implement TUI-side support for rendering protocol-level plugin UI descriptions (`UiNode`, `UiEffect`) from Phase 1. No plugin runtime execution — strictly state, rendering, and tests.

## Design Decisions

### Dialog approach: `Dialog::Plugin { id: String }`
- Single generic variant, not one per plugin
- `PluginDialog` component stores `id`, `title`, and `Vec<String>` (pre-rendered lines)
- `DialogType::Plugin` added to the component-level enum
- `From<DialogType::Plugin> for Dialog::Plugin` conversion

### Rendering strategy
- `PluginUiRenderer::render_node()` — ratatui Frame-based rendering for all UiNode variants
- `PluginUiRenderer::node_to_lines()` — flat string conversion for tests, info-dialog fallback, and snapshot assertions
- Treat `Markdown` as wrapped text (not full markdown rendering)

### Effect application
- `App::apply_plugin_ui_effect(effect: UiEffect)` central routing method
- `EmitChat` → stored for Phase 3 (no chat seam in TUI without agent integration)
- `ShowToast` → `messages_state.toasts.info/warn/error/success`
- `OpenDialog` → store in `PluginUiState.dialogs`, open `Dialog::Plugin { id }`
- `CloseDialog` → remove from `PluginUiState.dialogs`, close if active
- `OpenPanel`/`UpdatePanel`/`ClosePanel` → store in `PluginUiState.panels` (no visible rendering in Phase 2)
- `AddStatusItem`/`UpdateStatusItem`/`RemoveStatusItem` → store in `PluginUiState.status_items` (no visible rendering in Phase 2)

### Precedence
- Plugin dialogs render only when no first-party modal is active (permission, question, security review)
- If a first-party modal is open, plugin dialog effects are queued in state but not rendered

## Files to Create

### 1. `src/tui/app/state/plugin_ui.rs`
State container for plugin-owned UI surfaces.

```rust
use std::collections::BTreeMap;
use crate::protocol::ui::{DialogSpec, PanelSpec, StatusItemSpec, UiEffect};

#[derive(Debug, Clone, Default)]
pub struct PluginUiState {
    pub dialogs: BTreeMap<String, DialogSpec>,
    pub panels: BTreeMap<String, PanelSpec>,
    pub status_items: BTreeMap<String, StatusItemSpec>,
    pub last_effect_error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PluginUiApplyResult {
    Applied,
    ChatRequested,
    ToastRequested,
    Ignored,
    Error(String),
}
```

Methods:
- `apply_effect(UiEffect) -> PluginUiApplyResult` — central mutation
- `clear_plugin(plugin_id: &str)` — remove all surfaces for a plugin
- `get_dialog(id: &str) -> Option<&DialogSpec>` — lookup for rendering

### 2. `src/tui/components/plugin_renderer.rs`
Renderer adapter: UiNode → ratatui widgets and flat lines.

```rust
pub struct PluginUiRenderer;

impl PluginUiRenderer {
    pub fn render_node(frame: &mut Frame, area: Rect, theme: &Arc<Theme>, node: &UiNode);
    pub fn node_to_lines(node: &UiNode) -> Vec<String>;
}
```

Supported nodes: Text, Markdown (as wrapped text), Code, Table, KeyValue, Progress, Container, Empty, Unsupported.

### 3. `src/tui/components/dialogs/plugin.rs`
Plugin dialog component implementing `Component` trait.

```rust
pub struct PluginDialog {
    id: String,
    title: String,
    lines: Vec<String>,
    scroll: usize,
    theme: Arc<Theme>,
}
```

Pattern: follows `InfoDialog` (scrollable, Escape closes).

## Files to Modify

### 4. `src/tui/app/state/mod.rs`
Add `pub mod plugin_ui;` and `pub use plugin_ui::{PluginUiState, PluginUiApplyResult};`

### 5. `src/tui/app/mod.rs`
- Add `pub plugin_ui_state: PluginUiState` field to `App`
- Initialize in `with_config()` and `new_for_testing()`
- Add `pub fn apply_plugin_ui_effect(&mut self, effect: UiEffect) -> PluginUiApplyResult`
- Add `Plugin` variant handling in `open_dialog()` and `close_dialog()`

### 6. `src/tui/components/mod.rs`
Add `pub mod plugin_renderer;`

### 7. `src/tui/components/dialogs/mod.rs`
Add `pub mod plugin;`

### 8. `src/tui/app/types.rs`
Add `Plugin { id: String }` variant to `Dialog` enum. Update `is_open()`.

### 9. `src/tui/components/component.rs`
Add `Plugin` variant to `DialogType` enum. Update `From<DialogType> for Dialog` and `is_modal()`.

### 10. `src/tui/app/state/dialog.rs`
No changes needed — plugin dialogs are stored in `PluginUiState`, not `DialogState`.

## Tests

### Unit tests in `plugin_ui.rs`
- Dialog open/update/close via apply_effect
- Panel open/update/close via apply_effect
- Status item add/update/remove via apply_effect
- Toast effect produces correct result
- EmitChat produces ChatRequested
- Malformed/duplicate IDs do not panic
- clear_plugin removes all surfaces

### Unit tests in `plugin_renderer.rs`
- node_to_lines for Text, Markdown, Code, Table, KeyValue, Progress, Container, Empty, Unsupported
- Container with children renders all
- Nested containers
- Table with empty rows
- Progress without total
- Unsupported with unknown kind

### Handler tests in `mod.rs` (or new test module)
- apply_plugin_ui_effect routes OpenDialog correctly
- apply_plugin_ui_effect routes ShowToast correctly
- apply_plugin_ui_effect routes CloseDialog correctly
- Plugin dialog does not open when first-party modal is active

## Implementation Order

1. Create `src/tui/app/state/plugin_ui.rs` with state types and tests
2. Create `src/tui/components/plugin_renderer.rs` with renderer and tests
3. Add `Dialog::Plugin` to types.rs and `DialogType::Plugin` to component.rs
4. Create `src/tui/components/dialogs/plugin.rs`
5. Wire into App (field, constructors, apply_plugin_ui_effect, open/close)
6. Register modules in mod.rs files
7. Run `cargo ck` and `cargo test`
8. Update docs: AGENTS.md, architecture/plugin.md, architecture/tui.md, relevant skills

## Documentation Updates

### AGENTS.md
- Update "Plugin protocol is phase 1 only" note to reflect Phase 2 consumption
- Add PluginUiState, PluginUiRenderer to TUI section
- Add Dialog::Plugin variant to Dialog enum list

### architecture/plugin.md
- Update "Protocol DTOs (Phase 1)" section to note Phase 2 TUI consumption
- Add brief mention of PluginUiState and PluginUiRenderer

### architecture/tui.md
- Add plugin_ui.rs to directory structure
- Add plugin_renderer.rs to directory structure
- Add Dialog::Plugin to Dialog Variants list
- Add plugin.rs to dialogs list
- Update Dialog enum listing

### Skills
- `.opencode/skills/tui/SKILL.md` — add plugin renderer notes if relevant
- `.opencode/skills/plugin/SKILL.md` — add Phase 2 consumption notes

## Non-Goals (confirmed from plan)
- No plugin runtime execution
- No WASM/process code
- No native Component trait exposure to plugins
- No plugin key handlers or draw callbacks
- No panel/status visible rendering (stored only)
- No EmitChat wiring (deferred to Phase 3)

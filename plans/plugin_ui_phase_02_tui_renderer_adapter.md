# Phase 2 Plan: TUI Renderer Adapter for Portable UI Nodes

## Objective

Add TUI-side support for applying and rendering protocol-level plugin UI descriptions. This phase consumes `codegg_protocol::ui::{UiNode, UiEffect}` from Phase 1 and lowers them into ratatui output without exposing ratatui APIs to plugin authors.

No plugin runtime execution should be added in this phase. The work is strictly about TUI state, effect application, rendering, graceful degradation, and tests.

## Architectural Position

`codegg-protocol` owns the portable UI schema. The TUI owns presentation. This phase adds the adapter layer between them:

```text
Plugin/Protocol UiEffect -> TUI PluginUiState -> ratatui renderer -> terminal
```

The TUI’s native `Component` trait remains a first-party implementation detail. Plugin authors do not implement it.

## Files to Add

### `src/tui/app/state/plugin_ui.rs`

Add a state container for plugin-owned UI surfaces.

Recommended starting shape:

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

impl PluginUiState {
    pub fn apply_effect(&mut self, effect: UiEffect) -> PluginUiApplyResult {
        // Mutate maps for open/update/close effects.
        // Return high-level result so caller can emit chat/toast separately.
    }

    pub fn clear_plugin(&mut self, plugin_id: &str) {
        // Optional: if IDs are namespaced by plugin, remove all matching surfaces.
    }
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

The exact result type can differ, but effect application should be centralized and testable.

### `src/tui/components/plugin_renderer.rs`

Add a renderer adapter that turns `UiNode` into ratatui content.

Initial supported nodes:

- `Text`;
- `Markdown` as wrapped text initially, not full markdown rendering;
- `Code` with language label and preserved content;
- `Table` with simple column formatting;
- `KeyValue`;
- `Progress` as text/progress gauge if convenient;
- `Container` as vertical composition;
- `Empty`;
- `Unsupported` as a warning text block.

Recommended API:

```rust
use ratatui::{layout::Rect, Frame};
use std::sync::Arc;

use crate::protocol::ui::UiNode;
use crate::tui::theme::Theme;

pub struct PluginUiRenderer;

impl PluginUiRenderer {
    pub fn render_node(frame: &mut Frame, area: Rect, theme: &Arc<Theme>, node: &UiNode) {
        // Lower node to ratatui widgets.
    }

    pub fn node_to_lines(node: &UiNode) -> Vec<String> {
        // Useful for tests, fallback dialogs, and snapshot-like assertions.
    }
}
```

The `node_to_lines` helper is valuable because ratatui frame snapshot tests are more expensive. Start with line conversion tests and only add integration rendering tests if the repo already has a pattern for them.

## Files to Modify

### `src/tui/app/state/mod.rs`

Export the new `plugin_ui` state module.

### `src/tui/app/mod.rs`

Add a field to `App`:

```rust
pub plugin_ui_state: PluginUiState,
```

Initialize it in `App::with_config` and testing constructors.

### `src/tui/components/mod.rs`

Export `plugin_renderer`.

### TUI render path

Locate the central render method and add plugin UI rendering at safe layers:

1. Status items should be integrated only if a small seam exists in `StatusBarWidget`; otherwise defer status rendering to a later patch and keep state support.
2. Plugin panels should be rendered only if the layout has an obvious panel/sidebar seam. If not, store panels and expose them through an info dialog or a future panel route.
3. Plugin dialogs should render after first-party dialogs or with an explicit precedence rule. Do not break permission/question dialogs.

Conservative Phase 2 rendering target:

- render plugin dialogs through a generic dialog wrapper;
- provide `PluginUiRenderer::node_to_lines` for fallback display;
- do not force panel/status integration if it would destabilize the layout.

### `src/tui/app/types.rs` or current dialog state types

Avoid adding one `DialogType` variant per plugin. If a generic plugin dialog variant is needed, add a single `Dialog::Plugin { id: String }` or store plugin dialog separately and render it independently.

If adding `Dialog::Plugin`, ensure it does not require every plugin dialog to become a fixed enum entry.

## Implementation Steps

1. Add `PluginUiState` with effect application and unit tests.
2. Add `PluginUiRenderer` with `node_to_lines` and basic ratatui rendering helpers.
3. Add `plugin_ui_state` to `App` and initialize it in normal/test constructors.
4. Add an internal helper on `App`, such as `apply_plugin_ui_effect(effect: UiEffect)`, that routes:
   - `EmitChat` into message/chat handling if a seam exists, otherwise returns a result for Phase 3;
   - `ShowToast` into `ToastManager`;
   - dialog/panel/status effects into `PluginUiState`.
5. Add rendering support for at least plugin dialog bodies.
6. Add tests for effect application and node line rendering.
7. Run TUI-related tests and a root compile/test pass.

## Rendering Rules

### Dialogs

Plugin dialogs are semantic, not native component callbacks. They should display title + rendered `UiNode` body. Keyboard handling can initially be minimal: Escape closes the dialog. Do not add plugin-provided key handlers in this phase.

### Panels

Store panels in state. Render only if the current layout has a low-risk slot. Otherwise defer visible panel rendering to a follow-up while keeping state and tests.

### Status items

Store status items. Actual status bar integration can be minimal or deferred. Do not let plugin status rendering disrupt existing token/model/session status output.

### Unsupported nodes

Render as a concise fallback:

```text
Unsupported plugin UI node: <kind>
```

Do not panic on unknown/unsupported content.

## Tests

Add tests covering:

- `PluginUiState` opens, updates, and closes dialogs;
- `PluginUiState` opens, updates, and closes panels;
- `PluginUiState` adds, updates, and removes status items;
- `PluginUiRenderer::node_to_lines` for text, markdown, code, table, key-value, progress, container, and unsupported nodes;
- applying a toast effect produces the correct toast level if this is wired in this phase;
- malformed/duplicate IDs do not panic.

If the repo has render panic recovery tests, add one small case proving plugin UI unsupported nodes do not trigger render panic.

## Acceptance Criteria

- TUI builds with the new protocol UI types.
- `App` has a plugin UI state container.
- `UiEffect` can be applied to TUI state through a single helper.
- At least dialog UI can be displayed or converted into an info-dialog-like fallback.
- Node rendering is graceful for all initial `UiNode` variants.
- No plugin code receives ratatui/crossterm types.
- Permission/question/security dialogs are not displaced by plugin dialogs without an explicit precedence rule.

## Non-Goals

- Do not execute plugin commands.
- Do not add process or WASM runtime code.
- Do not modify the dynamic command loader yet.
- Do not expose the native `Component` trait to plugins.
- Do not support plugin key handling or draw callbacks.
- Do not require full panel/status visual integration if it is risky.

## Risks and Mitigations

### Risk: Dialog precedence conflicts

Permission and question dialogs should remain higher priority than plugin dialogs. If precedence is unclear, plugin dialogs should wait or be rendered only when no first-party modal is active.

### Risk: Layout churn

Do not refactor the whole layout system. Use a conservative generic dialog first. Panels/status can be fully integrated after command execution proves useful.

### Risk: Markdown rendering complexity

Treat markdown as wrapped text for now. Full markdown styling can be added later.

## Handoff Notes for Phase 3

Phase 3 should route plugin command completion responses into `App::apply_plugin_ui_effect` or equivalent. If Phase 2 exposes only state helpers and `node_to_lines`, Phase 3 can still render stdout/JSON responses through existing messages/toasts/info dialogs while retaining the correct abstraction.

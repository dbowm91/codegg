# Phase 14 Plan: TUI Component Modularization Follow-Through

## Objective

Use the new portable `UiNode` renderer to reduce duplication across first-party informational TUI surfaces without attempting a full declarative rewrite of the TUI.

The plugin UI work introduced a reusable representation for tables, key-value lists, markdown/text, code blocks, containers, progress, dialogs, panels, and status items. This phase should selectively move existing read-only/info-heavy dialogs onto that shared representation so first-party and plugin-generated UI share rendering behavior.

## Non-Goal

Do not rewrite interactive components such as permission prompts, question dialogs, command palette, editor-like inputs, file diffs, or complex tree navigation into `UiNode` in this phase. Keep interactive/focus-heavy widgets as native components.

## Candidate Surfaces

Good candidates are read-only or mostly read-only:

- `/tui-stats` output;
- `/plugins` and `/plugin-info` from Phase 11;
- `/plugin-doctor` diagnostics;
- usage/cost/context summaries;
- task list summary where non-interactive;
- shell command detail summaries;
- model/provider info summaries;
- doctor report summaries;
- LSP capability/detail summaries where read-only;
- memory search results if currently just lines/table.

Poor candidates for this phase:

- permission dialog;
- question dialog;
- command palette;
- file diff/hunk viewer;
- source preview with focus/selection/edit semantics;
- security review workflows with actions;
- tree browser;
- shell interactive execution view.

## Architecture

Add a small first-party UI builder layer:

```text
Domain data -> UiNode builder -> PluginUiRenderer / GenericUiRenderer -> existing dialog/panel surface
```

This should not be plugin-specific. Consider renaming or aliasing renderer concepts so first-party code does not appear to depend on plugin code.

Options:

1. Keep `PluginUiRenderer` as the implementation and document that it is a generic `UiNode` renderer despite the name.
2. Rename to `UiNodeRenderer` and keep `PluginUiRenderer` as a compatibility alias.

Prefer option 2 if churn is manageable.

## Files to Add

### `src/tui/components/ui_node_renderer.rs`

If renaming is chosen, move or wrap `plugin_renderer.rs`:

```rust
pub struct UiNodeRenderer;

impl UiNodeRenderer {
    pub fn render_node(...);
    pub fn node_to_lines(node: &UiNode) -> Vec<String>;
}
```

Keep `plugin_renderer.rs` as:

```rust
pub use super::ui_node_renderer::UiNodeRenderer as PluginUiRenderer;
```

or update call sites in one pass.

### `src/tui/components/dialogs/ui_node.rs`

Add a generic dialog component for read-only `UiNode` content.

Recommended:

```rust
pub struct UiNodeDialog {
    id: String,
    title: String,
    body: UiNode,
    scroll: u16,
    theme: Arc<Theme>,
}
```

It should support:

- render body lines;
- scroll up/down/page;
- escape/close;
- stable focus behavior;
- no domain-specific actions.

If current `PluginDialog` already does this well, generalize it rather than duplicating.

### `src/tui/ui_builders/`

Add builder modules for first-party summaries:

```text
src/tui/ui_builders/mod.rs
src/tui/ui_builders/stats.rs
src/tui/ui_builders/usage.rs
src/tui/ui_builders/plugins.rs
src/tui/ui_builders/shell.rs
src/tui/ui_builders/lsp.rs
```

Start with only the modules actually migrated in this phase.

## Migration Targets

Migrate at least three existing informational surfaces.

Recommended first set:

1. `/tui-stats` — currently line-heavy, ideal for key-value/table/container.
2. `/plugins` / `/plugin-info` / `/plugin-doctor` if Phase 11 exists — validate plugin UI path with first-party data.
3. Shell command detail summary or usage/cost/context summary — good table/key-value cases.

Each migrated surface should produce a `UiNode` before rendering.

## Implementation Steps

1. Rename/wrap renderer to `UiNodeRenderer` if chosen.
2. Add generic `UiNodeDialog` or generalize existing plugin dialog.
3. Add builder for `/tui-stats`:
   - app state summary;
   - task registry summary;
   - shell handles summary;
   - background activity summary;
   - remote/core status.
4. Route `/tui-stats` through `UiNodeDialog`.
5. Add builder for plugin management views if Phase 11 implemented.
6. Add builder for shell detail or usage/cost/context summary.
7. Remove duplicate manual line/table formatting where replaced.
8. Keep snapshot tests or line-render tests for each builder.

## Renderer Hardening

Improve renderer behavior while migrating:

- table column width handling;
- very long cell truncation/wrapping;
- empty table rendering;
- key-value alignment;
- container titles;
- progress text fallback;
- unsupported node warning;
- no panic on nested containers;
- no terminal escape interpretation.

Do not add full markdown rendering unless already available. Markdown-as-lines is acceptable.

## Tests

Add builder unit tests:

- stats builder includes expected sections;
- plugin info builder includes runtime/trust/capabilities;
- shell detail builder handles empty stdout/stderr;
- usage/cost builder handles missing values;
- table renderer handles uneven row width;
- nested container renders in stable order;
- long text does not panic;
- unsupported node falls back.

Add TUI tests:

- `/tui-stats` opens `UiNodeDialog`;
- migrated dialog scrolls;
- plugin dialogs still work after renderer rename/wrapper;
- permission/question/security dialogs remain unaffected;
- first-party and plugin `UiNode` rendering share the same line conversion.

## Compatibility

Keep old user-facing command behavior. The only intended difference is cleaner formatting and shared rendering.

If a migrated dialog previously produced model-visible chat content, preserve that behavior explicitly or document the change. Most informational dialogs should remain UI-only.

## Documentation Updates

Update `architecture/tui.md`:

- `UiNode` is now used for plugin and selected first-party read-only views;
- interactive components remain native ratatui components;
- `UiNodeRenderer` is the shared lowering adapter;
- `UiNodeDialog` is the generic scrollable read-only dialog.

Update `architecture/plugin.md` if renderer names changed.

## Acceptance Criteria

- At least three first-party informational surfaces render through `UiNode`.
- Existing plugin UI still works.
- Interactive/focus-heavy dialogs are not regressed.
- Shared renderer has tests for tables, key-values, containers, long text, and unsupported nodes.
- `/tui-stats` or an equivalent complex summary demonstrates the new path.
- Documentation explains what should and should not use `UiNode`.

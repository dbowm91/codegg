# TUI Phase 9: Layout and Render Regression Coverage

## Objective

Add systematic headless render coverage for the TUI using `ratatui::backend::TestBackend`. The current TUI has many moving parts: messages, sidebar, dialogs, completion overlays, shell detail views, research/security surfaces, toasts, and dynamic file-change metadata. Future changes need protection against panics, layout overflow, bad terminal-size behavior, and regressions in component fallback behavior.

## Current Shape

The repo already has useful TUI unit tests and shell dispatch tests. Recent work also added component-level render panic guards. The missing piece is a broad set of render-state fixtures that exercise realistic app states across terminal sizes without requiring an interactive terminal.

## Goals

- Render core TUI states with `TestBackend` at multiple dimensions.
- Assert no panics and no root render fallback for normal states.
- Verify known labels or fallback text appears where expected.
- Cover pathological content: long lines, wide Unicode, malformed JSON-like text, huge tool output summaries, empty messages, tool-only messages, and deep session/sidebar state.
- Add tests for component-level fallback behavior where panic injection is available.

## Test Matrix

Use a small matrix of terminal sizes:

- tiny: `40x12`
- small: `60x20`
- normal: `100x32`
- wide: `160x40`
- tall: `100x60`

Not every fixture must run every size if test runtime becomes high, but the base smoke states should.

## Core Fixtures

Create helper builders in `tests/tui_render.rs` or `src/tui/app/test_fixtures.rs` behind `#[cfg(test)]`.

Suggested helpers:

```rust
fn render_app_to_buffer(app: &mut App, width: u16, height: u16) -> ratatui::buffer::Buffer;
fn assert_render_ok(app: &mut App, width: u16, height: u16);
fn text_in_buffer(buffer: &Buffer) -> String;
```

Use:

```rust
let backend = TestBackend::new(width, height);
let mut terminal = Terminal::new(backend).unwrap();
terminal.draw(|frame| app.render(frame)).unwrap();
let buffer = terminal.backend().buffer().clone();
```

Exact API may vary by ratatui version.

## States to Cover

### 1. Empty/home state

- New testing app.
- No active session.
- Sidebar hidden and visible variants.
- Verify render does not panic at all matrix sizes.

### 2. Active session basic state

- Session route active.
- Prompt visible.
- Status bar visible.
- A few user/assistant messages.
- Verify recognizable text appears.

### 3. Streaming state

- Assistant streaming token content.
- `streaming_active = true`.
- Session status working.
- Spinner/status should render without panic.

### 4. Tool-call state

- Message list includes running tool call.
- Completed tool call.
- Failed tool call.
- Malformed or large tool output text represented as message content.

### 5. Sidebar states

- Sidebar visible.
- Changed files with diff states: pending, ready, skipped, error.
- Active goal and todo snippets if available.
- MCP servers list populated.
- Long file paths and Unicode paths.

### 6. Dialog states

Render each major dialog where possible:

- help
- model selector
- session selector
- tree dialog
- permission dialog
- question dialog
- import dialog
- share dialog
- shell show/info dialog
- research browser
- security review dialog
- memory/doctor/info surfaces if present

Each dialog test should assert render does not panic at small and normal sizes.

### 7. Completion overlay

- Slash completions visible.
- File completions visible.
- Long completion labels.
- Tiny terminal where overlay has limited room.

### 8. Search/timeline/toasts

- Search visible with matches and no matches.
- Timeline visible if available.
- Multiple toasts, including multi-line diagnostics toast.

### 9. Pathological text

- Very long unbroken line.
- Wide Unicode and emoji.
- Combining marks.
- ANSI-looking escape text in messages/tool output; ensure rendered safely as text.
- Large message list enough to require scrollbar.

## Component Panic Injection

If practical, add test-only flags to force a surface to panic:

```rust
#[cfg(test)]
pub struct RenderPanicInjection {
    pub messages: bool,
    pub sidebar: bool,
    pub dialog: bool,
    pub completions: bool,
}
```

Inject before the guarded render boundary, not deep inside unrelated code. Tests should verify:

- messages panic renders `Messages render error`
- sidebar panic renders `Sidebar unavailable`
- dialog panic closes or hides dialog and records diagnostics
- completion panic hides completions and records diagnostics
- root render panic count does not increment for guarded component panics
- component panic diagnostics increment

If panic injection would clutter production code, keep it behind `#[cfg(test)]` and narrow methods.

## Snapshot Policy

Avoid brittle full-buffer golden snapshots initially. Prefer semantic assertions:

- render succeeded
- buffer contains expected label/title
- buffer does not contain root render error text for normal states
- diagnostics counters changed only when expected

Full snapshots can be added later for stable high-value surfaces.

## Test Organization

Recommended file layout:

- `tests/tui_render.rs` for integration-style render tests
- `src/tui/app/render_tests.rs` for app-local private-state tests if needed
- fixture helpers under `#[cfg(test)]` to avoid exposing internals publicly

Keep the tests deterministic:

- avoid wall-clock-sensitive assertions
- disable background task spawning by using `App::new_for_testing`
- do not require a real core client
- do not require terminal raw mode

## Acceptance Criteria

- Headless render tests cover home/session/sidebar/dialog/completion/shell/research or security surfaces.
- Tests run at multiple terminal sizes, including small/tiny.
- Pathological message/tool text does not panic rendering.
- Component fallback behavior is tested for at least messages, sidebar, dialog, and completions if panic injection is added.
- Tests avoid brittle full-screen snapshots unless explicitly justified.
- Workspace checks pass.

## Out of Scope

- Pixel-perfect/golden UI assertions for every screen.
- Real terminal integration tests.
- Mouse interaction tests.
- Remote frontend render tests.

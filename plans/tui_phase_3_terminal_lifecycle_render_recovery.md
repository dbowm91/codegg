# TUI Phase 3: Terminal Lifecycle and Render Recovery Hardening

## Objective

Make terminal setup/teardown robust and reduce state loss from render failures. The TUI should restore the terminal reliably after setup errors, runtime errors, normal exit, and panics. Render failures should degrade only the failing surface where possible rather than resetting broad app state.

## Current Problem

The current terminal lifecycle uses free functions that enter alternate screen, enable raw mode, bracketed paste, and mouse capture, then later call `exit_raw()`. The teardown path manually prints the alternate-screen leave escape and also executes `LeaveAlternateScreen`. If setup partially fails, there is no guard that rolls back only the capabilities that were successfully enabled.

Render panic handling catches panics around `terminal.draw`, tracks a panic count, renders an error state, and after repeated failures resets broader app state. This is useful as an emergency fallback, but a single bad component should not force closing dialogs, clearing command mode, or resetting unrelated UI state.

## Design Direction

Introduce a `TerminalGuard` that owns terminal lifecycle state. The guard should track each terminal feature that was successfully enabled and restore exactly once in reverse order on drop or explicit shutdown.

Also introduce component-level render fallbacks around the riskiest surfaces. The root draw should remain protected, but failures in messages, sidebar, dialogs, completion overlay, or toasts should display a degraded fallback and log diagnostic details rather than repeatedly panicking the whole frame.

## TerminalGuard Design

Create a module such as `src/tui/terminal.rs` with a guard:

```rust
pub struct TerminalGuard {
    raw_enabled: bool,
    alt_screen: bool,
    bracketed_paste: bool,
    mouse_capture: bool,
    restored: bool,
}
```

Suggested API:

```rust
impl TerminalGuard {
    pub fn enter() -> Result<Self, AppError>;
    pub fn restore(&mut self);
}

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        self.restore();
    }
}
```

`enter()` should enable features step by step. If a later step fails, call `restore()` before returning the error. Avoid double-leaving alternate screen. Prefer `execute!(stdout(), LeaveAlternateScreen)` over manual escape printing unless there is a measured terminal-specific reason to keep the manual escape.

## Recommended Setup Order

A safe order is:

1. Enable raw mode.
2. Enter alternate screen.
3. Enable bracketed paste.
4. Enable mouse capture.
5. Create `ratatui::Terminal`.

Teardown should reverse this order:

1. Disable mouse capture if enabled.
2. Disable bracketed paste if enabled.
3. Leave alternate screen if entered.
4. Disable raw mode if enabled.

The exact order can be adjusted after manual testing, but it must be consistent and tracked.

## Event Loop Integration

Replace `enter_raw()?` and `exit_raw()` in `run_event_loop` with `TerminalGuard`:

```rust
pub async fn run_event_loop(app: &mut app::App) -> Result<(), AppError> {
    let mut terminal_guard = TerminalGuard::enter()?;
    let mut terminal = create_terminal()?;
    // loop
    terminal_guard.restore();
    Ok(())
}
```

Because `Drop` restores automatically, explicit restore is optional but useful for logging failures before returning.

Remove or restrict the old `enter_raw` and `exit_raw` functions after migration. If kept for compatibility, implement them through `TerminalGuard` or mark them internal.

## Render Recovery Design

Add a small safe-render helper:

```rust
fn safe_render_component<F>(name: &'static str, area: Rect, frame: &mut Frame, render: F) -> bool
where
    F: FnOnce(&mut Frame),
{
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| render(frame))) {
        Ok(()) => true,
        Err(err) => {
            render_component_error(frame, area, name, err);
            false
        }
    }
}
```

Ratatui rendering APIs may need slight adjustment because `Frame` borrowing can make a generic helper awkward. If so, implement explicit local `catch_unwind` blocks around each risky surface inside `App::render`.

## Components to Protect First

Prioritize surfaces that render dynamic or untrusted-sized content:

- `MessagesWidget`
- `SidebarWidget`
- active dialogs
- completion overlay
- research browser
- security review dialog
- shell-related display if present

Header/footer/status bar are likely lower risk and can remain in the root render path initially.

## Fallback Rendering

Fallback content should be compact and obvious:

- Messages failure: render a bordered block titled `Messages render error` with a short diagnostic.
- Sidebar failure: render `Sidebar unavailable` and continue main view.
- Dialog failure: close or hide only that dialog after rendering a toast/error.
- Completion overlay failure: hide completions and continue.

Do not place full panic payloads in the terminal if they are long. Log full detail through `tracing`; show a short surface-level message in the UI.

## Panic Count Semantics

Keep the existing root render panic count for failures outside component wrappers or terminal draw errors. Change the threshold behavior:

1. First root failure: render error screen and log.
2. Repeated root failures: hide optional overlays/dialogs and retry.
3. Final fallback: reset minimal volatile UI state only.

Avoid calling broad `app.reset_state()` unless absolutely necessary. If that behavior remains, log a high-severity warning that includes the last active dialog, route, and terminal size.

## Testing Plan

Unit tests:

1. `TerminalGuard::restore()` is idempotent. This can be tested by constructing a guard with booleans false/true in a test-only constructor and calling restore twice.
2. Partial setup rollback logic is covered with an injected terminal backend or trait if feasible. If crossterm injection is too invasive, cover idempotence and manually test partial failure.
3. Component fallback helper catches panic and returns false.
4. Render fallback for messages/sidebar/dialog does not panic when the inner renderer panics. This may require test-only panic-injecting widget hooks.

Headless render tests:

1. Render normal app at small, normal, and wide sizes.
2. Render with active dialog and completion overlay.
3. Render with long message text and sidebar visible.

Manual verification:

1. Start and quit the TUI normally; terminal returns to normal mode.
2. Force an early setup error if practical and verify terminal is restored.
3. Panic-inject a component and verify the rest of the frame renders.
4. Test inside `zellij`, plain terminal, and at least one terminal with bracketed paste enabled.

## Acceptance Criteria

- Terminal restoration is guard-owned and idempotent.
- Manual alternate-screen escape duplication is removed unless explicitly justified.
- Setup failure rolls back successfully enabled terminal features.
- Component-level render failures show degraded UI instead of broad app reset where practical.
- Root render panic recovery remains as final fallback.
- `cargo fmt --check`, `cargo clippy --workspace --all-targets --all-features -- -D warnings`, and `cargo test --workspace --all-features` pass.

## Out of Scope

- Replacing ratatui.
- Implementing a full crash reporter.
- Snapshot testing every widget. Broader render regression coverage belongs in Phase 9.

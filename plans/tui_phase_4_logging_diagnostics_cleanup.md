# TUI Phase 4: Logging and Diagnostics Cleanup

## Objective

Remove normal-operation ad hoc debug-file writes from the TUI and replace them with structured, low-overhead diagnostics through `tracing`. The TUI should not create or append `codegg_debug.log` in the user's current project directory unless an explicit debug mode requests it.

## Current Problem

Most TUI debug logging is feature-gated, but `src/tui/mod.rs` contains an unconditional `debug_log!` macro that opens and appends to `codegg_debug.log`. This file write can occur in frequent event paths. It pollutes user repositories, adds unnecessary filesystem churn, and makes logging behavior inconsistent across TUI modules.

The current diagnostics also do not clearly answer the questions that matter when the TUI feels sluggish: which command was running, how long rendering took, whether bus events were dropped, whether a core request stalled, and which UI surface was active.

## Design Direction

1. Remove unconditional file logging from normal builds.
2. Route debug events through `tracing` with structured fields.
3. Add targeted TUI runtime diagnostics for loop stalls, slow commands, render duration, dropped event bursts, and active dialog/surface.
4. Optionally add a user-facing diagnostic command such as `/doctor tui` or `/tui-stats` after the internal data exists.

## Logging Policy

Normal mode:

- No direct file writes from TUI macros.
- Use `tracing::debug!`, `tracing::info!`, `tracing::warn!`, and `tracing::error!`.
- Do not log every keypress by default.

Debug feature mode:

- Key/event debug logging may be enabled under `debug-logging`.
- If a file sink is used, it should write to a configured app data/cache path, not the project working directory.
- A clearly documented config or environment variable should control file output.

Suggested environment variables if a file sink already fits project conventions:

- `CODEGG_TUI_DEBUG=1`
- `CODEGG_TUI_DEBUG_LOG=/path/to/log`

Do not invent a new config system if existing tracing setup already supports this.

## Implementation Steps

### 1. Remove the unconditional macro in `src/tui/mod.rs`

Replace it with a feature-gated version matching the other TUI modules or replace call sites directly with `tracing`.

Preferred replacement:

```rust
#[cfg(feature = "debug-logging")]
macro_rules! debug_log {
    ($($arg:tt)*) => {
        tracing::debug!(target: "codegg::tui", "{}", format!($($arg)*));
    };
}

#[cfg(not(feature = "debug-logging"))]
macro_rules! debug_log {
    ($($arg:tt)*) => {};
}
```

If some messages should remain in normal builds, convert them to explicit `tracing::warn!` or `tracing::info!` at the call site.

### 2. Add TUI diagnostic state

Add a small diagnostic ring buffer in UI state or a dedicated runtime diagnostics struct. Keep it bounded.

Suggested fields:

```rust
pub struct TuiDiagnostics {
    pub slow_loop_count: u64,
    pub slow_render_count: u64,
    pub slow_command_count: u64,
    pub dropped_bus_events: u64,
    pub last_slow_loop: Option<SlowLoopRecord>,
    pub recent_slow_commands: VecDeque<SlowCommandRecord>,
}
```

The diagnostics should not allocate heavily on every frame. Record only when thresholds are crossed.

### 3. Instrument render duration

Around `render_app`, measure elapsed time. Suggested thresholds:

- Debug log if render exceeds 16 ms while streaming.
- Warn if render exceeds 100 ms.

Include fields:

- `elapsed_ms`
- `streaming_active`
- `session_status`
- `dialog`
- `message_count`
- `sidebar_visible`
- terminal size if available

### 4. Instrument command duration

For command handlers still awaited directly after Phase 1, wrap with timing. For spawned tasks, log start/finish from the async task helper. Slow command warnings should include command name and elapsed duration.

Suggested threshold: warn at 250 ms for TUI-loop-bound work; debug at 100 ms for spawned work unless it is expected to be long.

### 5. Instrument event bus lag and coalescing

Existing code warns when the broadcast receiver lags. Accumulate `dropped_bus_events` and store the last dropped count. Also log coalesced event batch size at debug level if the batch is unusually large.

### 6. Add optional `/doctor tui` or `/tui-stats`

If command registration is straightforward, add a read-only diagnostic command that surfaces:

- slow loop count
- slow render count
- slow command count
- dropped bus events
- last slow command
- last render error
- render panic count
- active dialog
- current mode

This can initially be a toast if short. If long, prefer a simple info dialog. Do not block the phase on a polished UI surface.

## Testing Plan

Unit tests:

1. `debug_log!` in non-debug builds does not perform file writes. This may be hard to assert directly; at minimum remove the direct `OpenOptions` path from `src/tui/mod.rs`.
2. Diagnostics ring buffer caps size.
3. Slow command records include command name and elapsed duration.
4. Dropped event counter increments correctly.

Static/search verification:

1. Search for `codegg_debug.log` and confirm no unconditional TUI path writes it.
2. Search for `OpenOptions::new()` in TUI modules and confirm any file logging is feature-gated or intentional.

Manual verification:

1. Run TUI normally and confirm no `codegg_debug.log` appears in the project directory.
2. Enable debug logging and confirm logs flow through expected tracing sink.
3. Trigger slow fake core operations and confirm structured slow-command diagnostics appear.
4. Trigger event-bus lag in a test or stress run and confirm diagnostics increment.

## Acceptance Criteria

- Normal TUI execution does not append `codegg_debug.log` to the working directory.
- TUI debug output is feature-gated or routed through `tracing`.
- Render duration, command duration, event bus lag, and loop stalls are measurable.
- Slow-path warnings include enough structured context to diagnose sluggishness.
- Optional `/doctor tui` or `/tui-stats` exists if command surface work is small; otherwise diagnostics are available through logs and internal state.
- `cargo fmt --check`, `cargo clippy --workspace --all-targets --all-features -- -D warnings`, and `cargo test --workspace --all-features` pass.

## Out of Scope

- Full telemetry/export infrastructure.
- Persistent diagnostic storage.
- A polished diagnostics dashboard.
- Changing provider/core logging outside TUI-specific paths.

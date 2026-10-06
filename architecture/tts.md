# TTS Module

Text-to-speech output for CodeGG using the macOS `say` command.

## Purpose

Provides local speech synthesis for agent turn output. In embedded mode the
TTS engine lives in the TUI process; in remote-core mode TTS requests route
through the daemon's `NotificationRouter` / `AudioArbiter`.

## Where It Lives

- `src/tts/mod.rs` — engine implementation
- `src/tui/app/state/ui.rs:88, 90, 99` — TUI state fields (`tts`, `tts_enabled`, `tts_via_daemon`)
- `src/tui/app/mod.rs:8043, 8100` — TUI integration (`toggle_tts`, `stop_tts`); daemon routing flag set at `:861`
- `src/tui/runtime/app_events.rs:333-345` — auto-stop on agent finished
- `src/tui/command.rs:573` — `/tts` slash command registration

## How It Works

### Embedded Mode (default)

The `Tts` struct owns an `Arc<AtomicBool>` speaking flag plus an
`Arc<Mutex<Option<u32>>>` holding the pid of the `say` child it spawned. Both
are `Arc`-shared because every call site works through a clone: `speak()` runs
inside a spawned TUI task and `stop()` inside a different one, so both handles
must observe the same child. `speak()` spawns
`tokio::process::Command::new("say")` in its **own process group**
(`process_group(0)`), records the child pid, and waits for completion. `stop()`
takes the pid out of the mutex and sends `SIGTERM` to that process group (via
`nix::sys::signal::killpg`), falling back to the bare pid if grouping was not
established. It never pattern-kills, so a `say` the user started outside
CodeGG is untouched.

### Remote-Core Mode

When `AppMode::RemoteCore` is active, `tts_via_daemon` is set to `true`
(`src/tui/app/mod.rs:861`). Toggle and stop operations route through
`CoreClient` using `CoreRequest::NotificationSpeak` instead of local
`say` invocation. The daemon's `AudioArbiter` handles playback.

### Agent Finished Auto-Stop

On `AgentFinished`, the TUI checks if TTS is speaking and (in embedded
mode only) calls `tts.stop()` to prevent leftover speech
(`src/tui/runtime/app_events.rs:333-345`).

## Key Types & APIs

### Tts (`src/tts/mod.rs:26`)

```rust
pub struct Tts {
    speaking: AtomicBool,
}
```

Methods:

| Method | Signature | Notes |
|--------|-----------|-------|
| `new()` | `-> Self` | Speaking flag starts `false` |
| `init()` | `fn(&mut self, TtsProvider) -> Result<(), AppError>` | Only handles `TtsProvider::None` (no-op) |
| `speak()` | `async fn(&self, &str)` | Validates non-empty; spawns `say`; sets flag |
| `stop()` | `async fn(&self) -> Result<(), AppError>` | Early return if not speaking; takes the tracked pid and signals that process group (`SIGTERM`) |
| `is_speaking()` | `fn(&self) -> bool` | Reads atomic flag |

`Clone` is implemented: clones the atomic flag value (not the process).

### TtsEngine Trait (`src/tts/mod.rs:20`)

```rust
#[async_trait]
pub trait TtsEngine: Send + Sync {
    async fn speak(&self, text: &str) -> Result<(), AppError>;
    async fn stop(&self) -> Result<(), AppError>;
    fn is_speaking(&self) -> bool;
}
```

`Tts` implements `TtsEngine` (delegates to inherent methods).

### TtsProvider (`src/tts/mod.rs:10`)

```rust
pub enum TtsProvider { None }
```

Only variant. The enum exists as a placeholder for future provider expansion.

## Configuration Surface

There is no `[tts]` config section. TTS has no voice, rate, or provider
configuration options. State is managed in-memory:

- `UiState.tts_enabled` — toggle state
- `UiState.tts_via_daemon` — routes through daemon in remote mode
- `UiState.tts` — the `Tts` engine instance

## Invariants & Gotchas

- **macOS-only**: hardcoded to `say` command. Cross-platform not implemented.
- **`stop()` only ever signals CodeGG's own child**: the pid is taken from the
  mutex under lock and the whole entry is cleared, so a concurrent `stop()`
  cannot double-signal. When no pid is tracked (idle, or `speak()` interrupted
  before recording it) `stop()` returns `Ok` without signalling anything —
  there is deliberately no pattern-kill fallback. `ESRCH` (child already
  exited) is treated as success.
- **Clones share state, deliberately**: `speaking` and `pid` are `Arc`-shared.
  This is load-bearing, not incidental — `speak()` and `stop()` are invoked from
  different spawned tasks holding different clones of the same logical
  speaker. Per-clone pid state would make `stop()` unable to find the child.
- **Speaking flag reset on spawn failure**: if `tokio::process::Command`
  fails to spawn, the flag is cleared in the error path
  (`src/tts/mod.rs:69-72`).
- **No daemon TTS when embedded**: `tts_via_daemon` is `false` in embedded
  mode; the TUI always speaks locally.
- **Auto-stop skips remote mode**: the `AgentFinished` handler only calls
  local `tts.stop()` when NOT in `RemoteCore` mode
  (`src/tui/runtime/app_events.rs:335-339`).

## Keybindings

| Key | Action |
|-----|--------|
| `Ctrl+Y` | Toggle TTS (speak selected message) |
| `Ctrl+Shift+Y` | Stop TTS playback |

Slash command: `/tts` (alias `/voice`).

## Related Docs

- [tui.md](tui.md) — TUI integration details
- [server.md](server.md) — daemon `NotificationRouter` for remote TTS

## Source verification

Verified 2026-10-06 against `src/tts/mod.rs` (single 125-line module):
`TtsProvider` (`:10`, only variant `None`), `TtsEngine` (`:20`),
`Tts` (`:26`) with `Arc`-shared `speaking`/`pid` state,
the spawn-error flag reset, the `process_group(0)` setup plus pid recording in
`speak()`, and the `killpg`-with-pid-fallback `stop()` that replaced the old
pattern kill — together with the 6 regression tests in `src/tts/mod.rs`
(`stop_when_idle_is_a_no_op`, `stop_without_tracked_child_does_not_signal`,
`stop_with_stale_pid_tolerates_esrch`, `clone_does_not_inherit_child_pid`,
`new_tts_is_idle_and_owns_no_child`, `speak_rejects_empty_text`), and the
`UiState` fields (`src/tui/app/state/ui.rs:88, 90, 99`), `toggle_tts`/
`stop_tts` (`src/tui/app/mod.rs:8043, 8100`), the `tts_via_daemon = true`
assignment (`src/tui/app/mod.rs:861`), the `AgentFinished` auto-stop with
its embedded-mode-only guard (`src/tui/runtime/app_events.rs:333-345`),
`/tts` registration (`src/tui/command.rs:573`), and the default bindings
`Ctrl+y` → `ToggleTts` / `Ctrl+Shift+Y` → `StopTts`
(`src/tui/input.rs:552-562`). The earlier `pkill say` caveat is **resolved**:
`stop()` now signals only the recorded child pid's process group.

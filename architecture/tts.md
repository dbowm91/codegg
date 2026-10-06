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

The `Tts` struct owns an `AtomicBool` speaking flag. `speak()` spawns
`tokio::process::Command::new("say")` with the text as an argument and waits
for completion. `stop()` uses `pkill say` to terminate the child process.

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
| `stop()` | `async fn(&self) -> Result<(), AppError>` | Early return if not speaking; `pkill say` |
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
- **`pkill say` is blunt**: stops ALL `say` processes, not just the one
  spawned by CodeGG.
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
`Tts` (`:26`) with `speaking: AtomicBool` and no `Mutex`, the spawn-error
flag reset (`:69-72`), `pkill say` in `stop()` (`:85-94`), and the
`UiState` fields (`src/tui/app/state/ui.rs:88, 90, 99`), `toggle_tts`/
`stop_tts` (`src/tui/app/mod.rs:8043, 8100`), the `tts_via_daemon = true`
assignment (`src/tui/app/mod.rs:861`), the `AgentFinished` auto-stop with
its embedded-mode-only guard (`src/tui/runtime/app_events.rs:333-345`),
`/tts` registration (`src/tui/command.rs:573`), and the default bindings
`Ctrl+y` → `ToggleTts` / `Ctrl+Shift+Y` → `StopTts`
(`src/tui/input.rs:552-562`). The `pkill say` caveat is retained: it
terminates every `say` process on the host, not just CodeGG's child.

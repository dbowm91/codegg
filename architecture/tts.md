# TTS Module

Text-to-speech output for CodeGG using the macOS `say` command.

## Purpose

Provides local speech synthesis for agent turn output. In embedded mode the
TTS engine lives in the TUI process; in remote-core mode TTS requests route
through the daemon's `NotificationRouter` / `AudioArbiter`.

## Where It Lives

- `src/tts/mod.rs` — engine implementation
- `src/tui/app/state/ui.rs:82-93` — TUI state fields (`tts`, `tts_enabled`, `tts_via_daemon`)
- `src/tui/app/mod.rs:9820-9921` — TUI integration (`toggle_tts`, `stop_tts`, daemon routing)
- `src/tui/runtime/app_events.rs:313-325` — auto-stop on agent finished
- `src/tui/command.rs:176-178` — `/tts` slash command registration

## How It Works

### Embedded Mode (default)

The `Tts` struct owns a `Mutex<AtomicBool>` speaking flag. `speak()` spawns
`tokio::process::Command::new("say")` with the text as an argument and waits
for completion. `stop()` uses `pkill say` to terminate the child process.

### Remote-Core Mode

When `AppMode::RemoteCore` is active, `tts_via_daemon` is set to `true`
(`src/tui/app/mod.rs:1378`). Toggle and stop operations route through
`CoreClient` using `CoreRequest::NotificationSpeak` instead of local
`say` invocation. The daemon's `AudioArbiter` handles playback.

### Agent Finished Auto-Stop

On `AgentFinished`, the TUI checks if TTS is speaking and (in embedded
mode only) calls `tts.stop()` to prevent leftover speech
(`src/tui/runtime/app_events.rs:313-325`).

## Key Types & APIs

### Tts (`src/tts/mod.rs:22`)

```rust
pub struct Tts {
    speaking: Mutex<std::sync::atomic::AtomicBool>,
}
```

Methods:

| Method | Signature | Notes |
|--------|-----------|-------|
| `new()` | `-> Self` | Speaking flag starts `false` |
| `init()` | `fn(&mut self, TtsProvider)` | Only handles `TtsProvider::None` (no-op) |
| `speak()` | `async fn(&self, &str)` | Validates non-empty; spawns `say`; sets flag |
| `stop()` | `async fn(&self) -> Result<(), AppError>` | Early return if not speaking; `pkill say` |
| `is_speaking()` | `fn(&self) -> bool` | Reads atomic flag |

`Clone` is implemented: clones the atomic flag value (not the process).

### TtsEngine Trait (`src/tts/mod.rs:16`)

```rust
#[async_trait]
pub trait TtsEngine: Send + Sync {
    async fn speak(&self, text: &str) -> Result<(), AppError>;
    async fn stop(&self) -> Result<(), AppError>;
    fn is_speaking(&self) -> bool;
}
```

`Tts` implements `TtsEngine` (delegates to inherent methods).

### TtsProvider (`src/tts/mod.rs:9`)

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
  (`src/tts/mod.rs:73-78`).
- **No daemon TTS when embedded**: `tts_via_daemon` is `false` in embedded
  mode; the TUI always speaks locally.
- **Auto-stop skips remote mode**: the `AgentFinished` handler only calls
  local `tts.stop()` when NOT in `RemoteCore` mode
  (`src/tui/runtime/app_events.rs:316-318`).

## Keybindings

| Key | Action |
|-----|--------|
| `Ctrl+Y` | Toggle TTS (speak selected message) |
| `Ctrl+Shift+Y` | Stop TTS playback |

Slash command: `/tts` (alias `/voice`).

## Related Docs

- [tui.md](tui.md) — TUI integration details
- [server.md](server.md) — daemon `NotificationRouter` for remote TTS

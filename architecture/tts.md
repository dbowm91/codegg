# TTS Module

The `tts` module provides text-to-speech functionality.

## Overview

**Location**: `src/tts/`

**Key Responsibilities**:
- Text-to-speech output
- Platform-specific implementation (macOS-only)

## Key Types

### Tts

```rust
pub struct Tts {
    speaking: std::sync::Mutex<std::sync::atomic::AtomicBool>,  // Thread-safe interior mutability
}

impl Clone for Tts { /* Uses Mutex for thread-safe cloning */ }

impl Default for Tts { /* Delegates to new() */ }

impl Tts {
    pub fn new() -> Self;
    pub fn init(&mut self, provider: TtsProvider) -> Result<(), AppError>;  // Only handles TtsProvider::None
    pub async fn speak(&self, text: &str) -> Result<(), AppError>;  // Validates non-empty text
    pub async fn stop(&self) -> Result<(), AppError>;  // Uses `pkill say` on macOS
    pub fn is_speaking(&self) -> bool;  // Returns current speaking state
}
```

**Notes**:
- `speak()` validates that `text` is non-empty, returning `Err(AppError::Io(...))` for empty strings
- `stop()` first checks if speaking, returns `Ok(())` early if not; otherwise uses `pkill say` on macOS to terminate ongoing speech
- `is_speaking()` returns `bool` (not `Result<bool, AppError>`)

### TtsEngine Trait

```rust
#[async_trait]
pub trait TtsEngine: Send + Sync {
    async fn speak(&self, text: &str) -> Result<(), AppError>;
    async fn stop(&self) -> Result<(), AppError>;
    fn is_speaking(&self) -> bool;
}
```

### TtsProvider

```rust
#[derive(Debug, Default)]
pub enum TtsProvider {
    #[default]
    None,  // Currently only supported provider
}
```

## Platform Support

### macOS

Uses the built-in `say` command via `tokio::process::Command`:

```rust
pub async fn speak(&self, text: &str) -> Result<(), AppError> {
    self.speaking.store(true, Ordering::SeqCst);
    let output = tokio::process::Command::new("say")
        .arg(text)
        .output()
        .await
        .map_err(AppError::Io)?;
    self.speaking.store(false, Ordering::SeqCst);
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        tracing::warn!("say command failed: {}", stderr);
        return Err(AppError::Io(std::io::Error::new(
            std::io::ErrorKind::Other,
            format!("say command failed: {}", stderr),
        )));
    }
    Ok(())
}
```

**Note**: Currently hardcoded to macOS `say` command. Cross-platform support not yet implemented.

## Error Handling

When `say` fails, `speak()` returns `Err(AppError::Io(...))` with the stderr message. Callers should handle these errors appropriately.

## Configuration

The TTS module has **no configuration integration**. There is no `[tts]` config section in the codebase.

- `init()` only handles `TtsProvider::None` (a no-op)
- TTS enabled state is managed in-memory in the UI layer (`tts_enabled` in `UiState`)
- No config options for voice, rate, or provider selection

## TUI Integration

TTS is integrated into the TUI with the following behaviors:

- **Auto-stop on AgentFinished**: When the agent finishes a turn, the TUI automatically calls `tts.stop()` if TTS is currently speaking (prevents leftover speech from a completed turn).
- **Toggle via `/tts` command**: The `/tts` slash command (alias: `/voice`) toggles TTS on/off. When enabled, it speaks the currently selected message text. When disabled, it stops playback.
- **Keybindings**: `Ctrl+Y` toggles TTS, `Ctrl+Shift+Y` stops TTS playback.
- **State management**: TTS enabled state is tracked in `UiState.tts_enabled` and the `Tts` instance lives in `UiState.tts`.

```rust
let tts = Tts::new();
tts.speak("Task completed successfully").await?;
```

## Keybindings

| Key | Action |
|-----|--------|
| `Ctrl+Y` | Toggle TTS (speak selected message) |
| `Ctrl+Shift+Y` | Stop TTS playback |

## See Also

- [.opencode/skills/tts/SKILL.md](../.opencode/skills/tts/SKILL.md) - TTS skill with UI integration details
- [tui.md](tui.md) - TUI notifications that could trigger TTS
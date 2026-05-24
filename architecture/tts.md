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
    speaking: std::sync::Mutex<std::sync::atomic::AtomicBool>,
}

impl Clone for Tts { /* Uses Mutex for thread-safe cloning */ }

impl Default for Tts { /* Delegates to new() */ }

impl Tts {
    pub fn new() -> Self;
    pub fn init(&mut self, provider: TtsProvider) -> Result<(), AppError>;
    pub async fn speak(&self, text: &str) -> Result<(), AppError>;
    pub async fn stop(&self) -> Result<(), AppError>;
    pub fn is_speaking(&self) -> bool;
}
```

**Note**: `speak()` validates that `text` is non-empty, returning `Err(AppError::Io(...))` for empty strings.

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

```toml
[tts]
enabled = true  # User preference, not implemented in code
```

**Note**: Configuration options like `voice` and `rate` mentioned in config are not implemented. The TTS module currently has no configuration integration - `init()` only handles `TtsProvider::None` which is a no-op.

## Usage

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
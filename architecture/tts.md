# TTS Module

The `tts` module provides text-to-speech functionality.

## Overview

**Location**: `src/tts/`

**Key Responsibilities**:
- Text-to-speech output
- Platform-specific implementation

## Key Types

### Tts

```rust
pub struct Tts {
    engine: Box<dyn TtsEngine>,
}

impl Tts {
    pub fn speak(&self, text: &str) -> Result<()>;
    pub fn stop(&self) -> Result<()>;
}
```

### TtsEngine Trait

```rust
pub trait TtsEngine: Send + Sync {
    fn speak(&self, text: &str) -> Result<()>;
    fn stop(&self) -> Result<()>;
}
```

## Platform Support

### macOS

Uses the built-in `say` command:

```rust
pub struct MacOSTtsEngine;

impl TtsEngine for MacOSTtsEngine {
    fn speak(&self, text: &str) -> Result<()> {
        Command::new("say")
            .arg(text)
            .spawn()?;
        Ok(())
    }
}
```

**Note**: Currently hardcoded to macOS `say` command. Cross-platform support not yet implemented.

## Usage

```rust
let tts = Tts::new();
tts.speak("Task completed successfully")?;
```

## Configuration

```toml
[tts]
enabled = true
voice = "Alex"  # macOS voice name
rate = 1.0       # Speech rate multiplier
```

## See Also

- [tui.md](tui.md) - TUI notifications that could trigger TTS

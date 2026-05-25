---
name: TTS
description: Text-to-speech module for audio output in opencode-rs
version: 1.0.0
tags: [tts, audio, speak]
---

Use the `/skill:TTS` command to load TTS context for text-to-speech capabilities.

## TTS Module

Location: `src/tts/mod.rs`

### Core Components

```rust
pub struct Tts {
    speaking: std::sync::Mutex<std::sync::atomic::AtomicBool>,
}

impl Clone for Tts { /* Uses Mutex for thread-safe cloning */ }

#[async_trait]
pub trait TtsEngine: Send + Sync {
    async fn speak(&self, text: &str) -> Result<(), AppError>;
    async fn stop(&self) -> Result<(), AppError>;
    fn is_speaking(&self) -> bool;
}

pub enum TtsProvider {
    None,  // Currently only supported provider
}
```

### Error Handling

When `say` fails, `speak()` now returns `Err(AppError::Io(...))` with the stderr message instead of silently ignoring the failure. Callers should handle these errors appropriately.

### TTS State in UI

The TUI stores TTS state in `UiState`:

```rust
// src/tui/app/state/ui.rs
pub struct UiState {
    // ... other fields
    pub tts: Tts,
    pub tts_enabled: bool,
}
```

### Keybindings

TTS has two keyboard shortcuts defined in `src/tui/input.rs`:

| Key | Action |
|-----|--------|
| `Ctrl+Y` | Toggle TTS (speak selected message) |
| `Ctrl+Shift+Y` | Stop TTS playback |

These can be customized via the keybind dialog.

### Footer Status

When TTS is enabled, the footer displays a speaker icon:

- `🔊 TTS` - TTS enabled
- `🔇 TTS` - TTS enabled but not speaking

### Usage in App

TTS is controlled via two methods in `src/tui/app/mod.rs`:

```rust
fn toggle_tts(&mut self) {
    self.ui_state.tts_enabled = !self.ui_state.tts_enabled;
    if self.ui_state.tts_enabled {
        if let Some(idx) = self.messages_state.messages.sel_msg {
            if let Some(msg) = self.messages_state.messages.get_message(idx) {
                let text = msg.text_content();
                if !text.is_empty() {
                    let tts = self.ui_state.tts.clone();
                    let text = text.clone();
                    tokio::spawn(async move {
                        if let Err(e) = tts.speak(&text).await {
                            tracing::debug!("TTS speak error: {}", e);
                        }
                    });
                }
            }
        }
        self.messages_state.toasts.info("TTS enabled");
    } else {
        let tts = self.ui_state.tts.clone();
        tokio::spawn(async move {
            if let Err(e) = tts.stop().await {
                tracing::debug!("TTS stop error: {}", e);
            }
        });
        self.messages_state.toasts.info("TTS disabled");
    }
}

fn stop_tts(&mut self) {
    let tts = self.ui_state.tts.clone();
    tokio::spawn(async move {
        if let Err(e) = tts.stop().await {
            tracing::debug!("TTS stop error: {}", e);
        }
    });
    self.messages_state.toasts.info("TTS stopped");
}
```

### Message Text Extraction

The `UIMessage` struct provides `text_content()` to extract speakable text:

```rust
// src/tui/components/messages.rs
impl UIMessage {
    pub fn text_content(&self) -> String {
        // Extracts text from all message parts (Text, Reasoning, ToolCall)
    }
}
```

### Platform Support

- **macOS**: Uses built-in `say` command
- **Linux**: Requires `speech-dispatcher` package (not yet implemented)

## Reference

- **TUI Development Guide**: `.skills/tui/SKILL.md`
- **Keybind Dialog**: `src/tui/components/dialogs/keybind.rs` handles TTS action bindings

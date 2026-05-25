# TTS Architecture Review (2026-05-25)

## Verified Correct Items

- `Tts` struct with `speaking: Mutex<AtomicBool>` (line 18-20)
- `TtsEngine` trait with `speak()`, `stop()`, `is_speaking()` (lines 11-16)
- `TtsProvider::None` as sole variant (lines 5-9)
- `speak()` validates non-empty text, returns `Err(AppError::Io)` for empty strings (lines 51-57)
- `speak()` returns `Err(AppError::Io)` when `say` command fails (lines 74-81)
- `is_speaking()` returns `bool`, not `Result<bool, AppError>` (lines 105-107)
- `stop()` uses `pkill say` on macOS (lines 93-94)
- `init()` only handles `TtsProvider::None` (lines 45-49)
- No `[tts]` config section exists
- `Tts` implements `Clone`, `Default`; has `new()` constructor
- `UiState` has `tts: Tts` and `tts_enabled: bool` (ui.rs:67-69)
- Keybindings: `Ctrl+Y` = ToggleTts, `Ctrl+Shift+Y` = StopTts (input.rs:312-321)
- Footer displays `🔊` when speaking, `🔇` when not speaking (footer.rs:307)
- `toggle_tts()` and `stop_tts()` exist in `src/tui/app/mod.rs:4228` and `:4257`
- `TtsEngine` impl for `Tts` correctly forwards to `Tts` methods (lines 110-123)

## Incorrect/Stale Items

### architecture/tts.md:30-31
**Issue**: `stop()` behavior description is misleading.

Current doc:
> pub async fn stop(&self) -> Result<(), AppError>;  // Uses `pkill say` on macOS

**Problem**: The doc at line 37 says "returns `Ok(())` even if no speech is running". Actual code at lines 86-88 short-circuits and returns early BEFORE running `pkill` if not speaking:
```rust
if !self.is_speaking() {
    return Ok(());
}
```

**Fix**: Update line 30-31 to clarify behavior, and line 37 should say:
> `stop()` returns `Ok(())` immediately if not speaking; otherwise uses `pkill say` and returns `Ok(())` even if pkill reports no process found

## Minor Documentation Issues (Low Priority)

### architecture/tts.md:28
**Issue**: The comment says `init()` "Only handles TtsProvider::None". While true, a match-with-no-wildcard would be cleaner, but current code is correct.

### skill: Line 54-62 (Keybindings section)
The skill says keybindings are "defined in `src/tui/input.rs`" which is technically accurate for the `InputAction` enum values, but the actual key-to-action mapping is in `default_keybindings()` function. Minor naming precision.

### skill: Line 130-132 (Linux note)
Says Linux "Requires `speech-dispatcher` package (not yet implemented)". This is forward-looking/ aspirational; actual implementation is macOS-only with no platform detection. Could be clarified but not strictly incorrect.

## No Bugs Found in Implementation

The TTS implementation is correct. No actual code bugs were identified.

## Summary

The architecture document is **90% accurate**. Only one meaningful issue: the `stop()` method description incorrectly implies `pkill` is always run even when not speaking. Everything else checks out.

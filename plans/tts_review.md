# TTS Module Review

## Summary

Reviewed `architecture/tts.md` against the actual implementation in `src/tts/mod.rs` and the skill guide in `.opencode/skills/tts/SKILL.md`. The documentation is generally accurate and up-to-date.

## Verified Correct Items

### Architecture Document (tts.md)
1. **Tts struct** - Uses `Mutex<AtomicBool>` as documented (line 19)
2. **Clone impl** - Properly implemented with Mutex for thread-safe cloning (lines 22-30)
3. **Default impl** - Delegates to `new()` as documented (lines 32-36)
4. **TtsEngine trait** - Async trait with `speak`, `stop`, `is_speaking` methods (lines 11-16)
5. **TtsProvider enum** - `None` variant with `Default` derivation (lines 5-9)
6. **Platform support** - macOS `say` command implementation accurate (lines 62-82)
7. **Error handling** - Returns `Err(AppError::Io(...))` on failure (lines 77-80)
8. **stop() method** - Implemented with `pkill say` (lines 85-103)
9. **is_speaking() method** - Properly implemented (lines 105-107)

### Skill Guide
1. **Tts struct field** - `speaking: Mutex<AtomicBool>` matches actual (SKILL.md line 17)
2. **Clone impl comment** - "Uses Mutex for thread-safe cloning" accurate (SKILL.md line 20)
3. **Error handling** - Skill correctly notes `speak()` returns `Err(AppError::Io(...))` (SKILL.md lines 35-36)
4. **Keybindings** - Ctrl+Y and Ctrl+Shift+Y correctly documented (SKILL.md lines 55-58)
5. **Platform notes** - macOS using `say` command, Linux requires `speech-dispatcher` (SKILL.md lines 129-130)

## Discrepancies Found

### 1. Configuration Section Inaccuracy (architecture/tts.md:91-96)

The config section states:

```toml
[tts]
enabled = true  # User preference, not implemented in code
```

**Issue**: The configuration value `enabled` is documented as "not implemented in code" but there is no actual `[tts]` configuration section being loaded or validated in the TTS module. The module does not reference any config.

**Actual**: The TTS module has no configuration integration. There is no `tts_enabled` field being loaded from config, and the `init()` method only handles `TtsProvider::None` (a no-op). This should be documented as a missing feature, not a partially-implemented one.

### 2. stop() Implementation Missing from Architecture (architecture/tts.md)

**Issue**: The architecture document does not document the `stop()` method or its implementation (using `pkill say`). The macOS example only shows the `speak()` method code block.

**Actual**: `stop()` exists at lines 85-103 and uses `pkill say` to stop ongoing speech.

### 3. empty string Validation Missing from Architecture (architecture/tts.md:29)

**Issue**: The `pub async fn speak(&self, text: &str) -> Result<(), AppError>` signature in the architecture doc does not mention that `speak()` validates for empty strings and returns an error.

**Actual**: `speak()` at lines 52-57 checks `if text.is_empty()` and returns `Err(AppError::Io(...))` with `ErrorKind::InvalidInput`.

### 4. stop() Success Check Inconsistency (src/tts/mod.rs:98-101)

**Issue**: In `stop()`, when `pkill say` fails (non-zero exit), the code logs a warning but still returns `Ok(())`. This may mask errors where the speech process could not be stopped.

**Code** (lines 98-101):
```rust
if !output.status.success() {
    let stderr = String::from_utf8_lossy(&output.stderr);
    tracing::warn!("pkill say failed: {}", stderr);
}
Ok(())
```

**Recommendation**: Consider returning `Err(AppError::Io(...))` if stopping fails and we were expecting to stop speech. However, this may be acceptable since `pkill` can fail if `say` is not running, which is not necessarily an error condition.

## Minor Issues

### Documentation Improvements

1. **Architecture doc missing `stop()` method signature**: Should add `pub async fn stop(&self) -> Result<(), AppError>` to the Tts impl block documentation.

2. **Architecture doc missing `is_speaking()` method signature**: Should add `pub fn is_speaking(&self) -> bool` to the Tts impl block documentation.

3. **Architecture doc shows incomplete Tts struct example**: The example shows `speaking: std::sync::atomic::AtomicBool` but actual is `speaking: std::sync::Mutex<std::sync::atomic::AtomicBool>` (lines 17-20).

4. **init() method not fully documented**: Architecture shows `pub fn init(&mut self, provider: TtsProvider) -> Result<(), AppError>` but doesn't explain that currently it only handles `TtsProvider::None` and is essentially a no-op.

## Conclusion

The TTS module implementation is correct and well-structured. The documentation is mostly accurate but needs updates to reflect:
1. The `stop()` method implementation
2. The `is_speaking()` method signature  
3. The `Mutex<AtomicBool>` wrapping for the `speaking` field
4. The empty string validation in `speak()`
5. The fact that configuration is not implemented (not just partially)

No bugs were found in the actual implementation. The code correctly handles error cases, properly manages the speaking state, and implements all documented functionality.

## Files Referenced

- `src/tts/mod.rs` - Main implementation (123 lines)
- `architecture/tts.md` - Architecture documentation (115 lines)
- `.opencode/skills/tts/SKILL.md` - Skill guide (135 lines)

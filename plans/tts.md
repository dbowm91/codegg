# TTS Architecture Review

## Architecture Document
- Path: architecture/tts.md

## Source Code Location
- src/tts/

## Verification Summary
Partial

## Verified Claims (table format)

| Claim | Status | Notes |
|-------|--------|-------|
| Tts struct with speaking field | Fail | Doc shows `AtomicBool`, actual is `Mutex<AtomicBool>` for Clone safety |
| Tts has new(), init(), speak(), stop(), is_speaking() | Partial | stop() missing from doc entirely |
| TtsEngine trait with speak/stop/is_speaking | Pass | Accurate |
| TtsProvider enum with None variant | Pass | Accurate |
| macOS uses "say" command | Pass | Accurate |
| Error handling returns AppError::Io | Pass | Accurate |
| Configuration not implemented | Pass | True - enabled = true noted but unused |
| Keybindings Ctrl+Y and Ctrl+Shift+Y | Pass | Verified in tui/input.rs |

## Issues Found

### Bugs

1. **Duplicate store(false) in speak() (lines 74-77)**
   - After `tokio::process::Command::new("say")` completes, `speaking` is set to false on lines 70-73, then set to false again on lines 74-77
   - This is redundant dead code
   - The second block (lines 74-77) appears to be a copy-paste error

2. **Empty text validation not in stop()**
   - `speak()` returns error for empty strings (lines 52-57)
   - `stop()` does not check `is_speaking()` before calling `pkill say`
   - If nothing is speaking, `stop()` still tries to kill the say process and may return an error

3. **init() silently accepts any provider**
   - `init()` matches on `TtsProvider::None` and returns `Ok(())`
   - If a different variant were passed, it would silently do nothing
   - Should return an error for unsupported providers

### Inconsistencies

1. **Tts.speaking type in doc vs actual**
   - Architecture doc: `speaking: std::sync::atomic::AtomicBool`
   - SKILL.md: `speaking: std::sync::atomic::AtomicBool`
   - Actual: `speaking: Mutex<std::sync::atomic::AtomicBool>`
   - The Mutex is required for Clone implementation but is undocumented

2. **stop() method not documented**
   - Architecture doc does not mention the `stop()` method at all
   - Actual implementation uses `pkill say` to stop playback
   - Error handling for `pkill` failure is not documented

3. **TtsEngine impl not documented**
   - `impl TtsEngine for Tts` (lines 115-128) delegates to Tts methods
   - This implementation is not shown in architecture

4. **Empty string validation undocumented**
   - `speak()` rejects empty strings with `InvalidInput` error
   - This behavior is not mentioned in architecture

### Missing Documentation

1. **`stop()` method** - Completely absent from architecture
2. **`pkill say` mechanism** - How stop works is undocumented
3. **Error conditions** - `pkill say` failing returns error, not documented
4. **Tts Clone implementation** - Uses Mutex wrapper for thread-safety
5. **TtsEngine trait implementation** - Pattern of delegating to Tts methods
6. **Linux platform status** - SKILL.md mentions Linux but code has no implementation

### Improvement Opportunities

1. **Add is_speaking guard to stop()** - Check before calling pkill to avoid error when nothing is speaking
2. **Return error on unknown provider** - `init()` should error if provider is not `None`
3. **Consider TtsError type** - Dedicated error type could be clearer than AppError::Io
4. **Document speak() empty string validation** - Add to architecture that empty strings are rejected
5. **Update SKILL.md** - Fix Tts.speaking type to show Mutex wrapper

## Recommendations

1. **Remove duplicate code** - Delete lines 74-77 in src/tts/mod.rs (duplicate `speaking.store(false, ...)`)
2. **Add stop() to architecture doc** - Document the method, its use of pkill, and error handling
3. **Fix Tts struct documentation** - Show `Mutex<AtomicBool>` instead of just `AtomicBool`
4. **Add is_speaking check to stop()** - Return early if not speaking instead of always trying pkill
5. **Error on unknown provider** - Change `init()` to return error for non-None providers
6. **Update SKILL.md** - Sync the Tts struct definition with actual implementation

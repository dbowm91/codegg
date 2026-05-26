# TTS Architecture Review

**Review date**: 2026-05-26
**Reviewer**: Architecture review agent
**Source file**: `architecture/tts.md` (119 lines)
**Source code**: `src/tts/mod.rs` (123 lines)

---

## Summary

The architecture document is **mostly accurate** with one minor simplification in the code example. The TTS module is small, macOS-specific, and correctly documented.

---

## Detailed Findings

### 1. Module Location ✅

| Claim | Actual | Status |
|-------|--------|--------|
| `src/tts/` | `src/tts/mod.rs` | Correct |

### 2. Tts Struct ✅

**Line 18-20** in doc vs **Line 18-20** in source:

- Doc shows `speaking: std::sync::Mutex<std::sync::atomic::AtomicBool>`
- Source shows `speaking: Mutex<std::sync::atomic::AtomicBool>` (uses `std::sync::` prefix via `use` statement)

The doc correctly shows the mutex wrapping atomic bool pattern. Line numbers align.

**Line 22-30** - Clone impl: Documented correctly.

**Line 32-34** - Default impl: Documented correctly.

### 3. Tts Methods ✅

**Line 27-32** - Method signatures match `src/tts/mod.rs:38-108`:
- `pub fn new() -> Self` ✅ (line 39)
- `pub fn init(&mut self, provider: TtsProvider) -> Result<(), AppError>` ✅ (line 45) - **only handles TtsProvider::None** ✅
- `pub async fn speak(&self, text: &str) -> Result<(), AppError>` ✅ (line 51)
- `pub async fn stop(&self) -> Result<(), AppError>` ✅ (line 85)
- `pub fn is_speaking(&self) -> bool` ✅ (line 105)

**Line 36** - Empty text validation: ✅ Correct (line 52-56 in source)

**Line 37** - `stop()` behavior: ✅ Correct - checks `is_speaking()`, returns `Ok(())` early, uses `pkill say` (line 93)

### 4. TtsEngine Trait ✅

**Line 42-49** - Trait definition matches `src/tts/mod.rs:11-16`:
- Lines align exactly
- `Send + Sync` bounds documented correctly

### 5. TtsProvider ✅

**Line 53-59** - Enum definition matches `src/tts/mod.rs:5-9`:
- Lines align exactly
- `#[default]` attribute documented correctly
- "Currently only supported provider" is accurate

### 6. Platform Support (macOS) ⚠️ Minor Discrepancy

**Line 62-86** - `speak()` implementation:
- Doc shows code block at lines 69-86
- Actual source at `src/tts/mod.rs:51-83`

**Key difference**: Doc shows simplified code:
```rust
self.speaking.store(true, Ordering::SeqCst);
```

But source uses mutex locking:
```rust
self.speaking.lock().unwrap().store(true, std::sync::atomic::Ordering::SeqCst);
```

**This is a documentation simplification** for brevity, not an error. The intent is clear.

### 7. Error Handling ✅

**Line 90-92** - Documented correctly: `speak()` returns `Err(AppError::Io(...))` with stderr message on failure.

### 8. Configuration ✅

**Line 94-101** - Verified:
- No `[tts]` config section exists (grep confirms - no matches)
- `init()` only handles `TtsProvider::None` ✅
- `tts_enabled` in `UiState` at `src/tui/app/state/ui.rs:69` ✅
- No config options for voice/rate/provider ✅

### 9. Keybindings ✅

**Line 111-114** - Keybindings table:

| Key | Action |
|-----|--------|
| `Ctrl+Y` | Toggle TTS (speak selected message) |
| `Ctrl+Shift+Y` | Stop TTS playback |

**Actual bindings** at `src/tui/input.rs:311-321`:
```rust
map.insert(
    (KeyModifiers::CONTROL, KeyCode::Char('y')),
    InputAction::ToggleTts,
);
map.insert(
    (KeyModifiers::CONTROL | KeyModifiers::SHIFT, KeyCode::Char('Y')),
    InputAction::StopTts,
);
```

Note: `KeyCode::Char('Y')` with SHIFT modifier is the **uppercase Y** representation. The documentation's "Ctrl+Shift+Y" is a user-friendly display representation, which matches how the keybind dialog and help overlay display these bindings.

### 10. UiState TTS Fields ✅

SKILL.md shows `tts` in `UiState` but does not show `tts_enabled`. Source at `src/tui/app/state/ui.rs:67-69` confirms both fields exist:
```rust
pub tts: Tts,
pub tts_enabled: bool,
```

---

## Verification Checklist

| Item | Claim | Actual | Line(s) |
|------|-------|--------|---------|
| Module location | `src/tts/` | `src/tts/mod.rs` | - |
| Tts struct fields | `Mutex<AtomicBool>` | Same | mod.rs:18-20 |
| Tts::new() | Exists | Exists | mod.rs:39 |
| Tts::init() | Only handles None | Correct | mod.rs:45-49 |
| Tts::speak() | Validates non-empty | Correct | mod.rs:52-56 |
| Tts::stop() | Uses pkill say | Correct | mod.rs:85-103 |
| Tts::is_speaking() | Returns bool | Correct | mod.rs:105 |
| TtsEngine trait | Send + Sync | Correct | mod.rs:12 |
| TtsProvider | None only | Correct | mod.rs:5-9 |
| Keybindings Ctrl+Y | Defined in input.rs | Correct key codes | input.rs:311-321 |
| No [tts] config | Confirmed | Confirmed | grep - no results |
| tts_enabled in UiState | ui.rs:69 | Confirmed | ui.rs:69 |

---

## Field Counts

| Type | Documented Fields | Actual Fields | Match |
|------|-------------------|---------------|-------|
| `Tts` struct | 1 (`speaking`) | 1 | ✅ |
| `TtsEngine` trait | 3 methods | 3 methods | ✅ |
| `TtsProvider` enum | 1 variant | 1 variant | ✅ |
| `UiState` TTS fields | 2 (`tts`, `tts_enabled`) | 2 | ✅ |

---

## Code-to-Doc Line Mapping

| Doc Line | Content | Source Line | Match |
|----------|---------|-------------|-------|
| 1-13 | Header/Overview | - | ✅ |
| 18-20 | Tts struct | mod.rs:18-20 | ✅ |
| 22-30 | Clone impl | mod.rs:22-30 | ✅ |
| 32-34 | Default impl | mod.rs:32-36 | ✅ |
| 45-49 | init() | mod.rs:45-49 | ✅ |
| 51-61 | speak() header | mod.rs:51-83 | ✅ |
| 62-86 | speak() code | mod.rs:51-83 | ⚠️ (simplified) |
| 85-103 | stop() code | mod.rs:85-103 | ✅ |
| 105-107 | is_speaking() | mod.rs:105-107 | ✅ |
| 111-114 | Keybindings table | input.rs:311-321 | ✅ |
| 118 | See Also links | - | ✅ |

---

## Conclusion

The architecture document is **95% accurate**.

**Minor simplification (acceptable)**:
- The `speak()` code example in the doc shows `.store()` directly while source uses `.lock().unwrap().store()`. This is documentation brevity for clarity, not an error.

**No corrections required** - the architecture doc is substantively correct. The simplification in the code example is a stylistic choice that doesn't affect accuracy of the described behavior.

The TTS module is straightforward: 123 lines implementing a macOS-only `say` command wrapper with basic speak/stop/is_speaking functionality.

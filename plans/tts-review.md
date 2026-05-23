# TTS Module Architecture Review

## Verified Claims (what matches)

| Claim | Status | Evidence |
|-------|--------|----------|
| `Tts` struct has `speaking: AtomicBool` field | VERIFIED | `src/tts/mod.rs:18` |
| `Tts` implements `Clone` correctly | VERIFIED | `src/tts/mod.rs:21-29` - clones the atomic value |
| `Tts` implements `Default` | VERIFIED | `src/tts/mod.rs:31-35` - delegates to `new()` |
| `Tts::new()` exists and returns `Self` | VERIFIED | `src/tts/mod.rs:38-42` |
| `Tts::init(&mut self, provider: TtsProvider)` signature | VERIFIED | `src/tts/mod.rs:44-48` - handles `TtsProvider::None` |
| `Tts::speak(&self, text: &str)` async method | VERIFIED | `src/tts/mod.rs:50-69` |
| `Tts::stop(&self)` async method | VERIFIED | `src/tts/mod.rs:71-79` - uses `pkill say` |
| `Tts::is_speaking(&self) -> bool` method | VERIFIED | `src/tts/mod.rs:81-83` |
| `TtsEngine` trait with `speak`, `stop`, `is_speaking` | VERIFIED | `src/tts/mod.rs:10-15` |
| `TtsProvider::None` with `#[default]` | VERIFIED | `src/tts/mod.rs:4-8` |
| Uses macOS `say` command via `tokio::process::Command` | VERIFIED | `src/tts/mod.rs:53` |
| Error handling returns `AppError::Io` on failure | VERIFIED | `src/tts/mod.rs:63-67` |
| `speak()` sets `speaking=true` before command | VERIFIED | `src/tts/mod.rs:51-52` |
| `speak()` sets `speaking=false` after command | VERIFIED | `src/tts/mod.rs:58-59` |
| Keybinding `Ctrl+Y` toggles TTS | VERIFIED | `src/tui/app/mod.rs:284` |
| Keybinding `Ctrl+Shift+Y` stops TTS | VERIFIED | `src/tui/app/mod.rs:285` |
| `Tts` implements `TtsEngine` trait | VERIFIED | `src/tts/mod.rs:86-99` |
| `toggle_tts()` spawns async task for speech | VERIFIED | `src/tui/app/mod.rs:4136-4164` |
| `stop_tts()` spawns async task to stop | VERIFIED | `src/tui/app/mod.rs:4165-4176` |

## Bugs/Discrepancies Found

### Critical (Correctness)

1. **`speak()` error path does not reset `speaking` flag** (line 57):
   - When `Command::new("say").output().await` fails (e.g., `say` command not found), `map_err(AppError::Io)?` returns early
   - `speaking` was already set to `true` at line 51-52
   - The flag is never reset to `false` on this error path
   - **Impact**: TTS gets stuck in "speaking" state permanently on any error that occurs before line 58-59
   - **Fix**: Use `map_err(|e| { self.speaking.store(false, Ordering::SeqCst); AppError::Io(e) })` or restructure to reset on all error paths

2. **Race condition between concurrent `speak()` and `stop()` calls**:
   - `speak()` at line 51-52 sets `speaking=true`, then awaits the command
   - `stop()` at line 72-73 sets `speaking=false`
   - If `stop()` is called during the `await` in `speak()`, the flag is incorrectly set after `speak()` completes
   - **Impact**: `is_speaking()` returns incorrect state during and after concurrent calls
   - **Fix**: Use a `Mutex` to serialize access to the speaking state

### Medium (Correctness)

3. **`stop()` ignores `pkill` failure silently** (line 74-77):
   - `let _ = tokio::process::Command::new("pkill").arg("say").output().await;`
   - Result is discarded; if `pkill` fails or no `say` process exists, this is silently ignored
   - **Impact**: Callers cannot tell if stop actually worked
   - **Fix**: Log warning when `pkill` fails and return error

4. **`speak()` behavior with empty string**:
   - `say ""` succeeds but produces no audio
   - The method returns `Ok(())` but nothing is spoken
   - **Impact**: Minor - unexpected but not harmful
   - **Fix**: Consider returning error or warning for empty text

### Low (Observability)

5. **`stop()` always returns `Ok(())`** (line 78):
   - Even if `pkill` fails to find/kill any process, `stop()` returns success
   - **Impact**: Callers cannot distinguish "nothing was playing" from "successfully stopped"
   - **Fix**: Return `Result<(), AppError>` based on `pkill` exit status

6. **No process lifecycle guarantee**:
   - `say` is spawned as child process with no mechanism to ensure it terminates if app exits
   - The `pkill` approach is external and may not work reliably in all scenarios
   - **Impact**: Potential zombie processes or speech continuing after app exit
   - **Fix**: Consider using process group or storing PID for targeted kill

## Improvement Suggestions (with priority)

### Priority: High

1. **Fix `speaking` flag reset on error path**:
   ```rust
   pub async fn speak(&self, text: &str) -> Result<(), AppError> {
       self.speaking.store(true, Ordering::SeqCst);
       let result = tokio::process::Command::new("say")
           .arg(text)
           .output()
           .await;
       self.speaking.store(false, Ordering::SeqCst);
       result.map_err(AppError::Io)?;
       if !output.status.success() { ... }
       Ok(())
   }
   ```

2. **Add synchronization for concurrent access**:
   - Replace `speaking: AtomicBool` with a proper state machine or `Mutex<SpeakingState>`
   - This fixes both the race condition and ensures `speaking` is reset on all error paths

3. **`stop()` should report `pkill` failures**:
   ```rust
   pub async fn stop(&self) -> Result<(), AppError> {
       self.speaking.store(false, Ordering::SeqCst);
       let output = tokio::process::Command::new("pkill")
           .arg("say")
           .output()
           .await
           .map_err(AppError::Io)?;
       if !output.status.success() {
           tracing::debug!("pkill say: no process found");
       }
       Ok(())
   }
   ```

### Priority: Medium

4. **Add voice/rate configuration to `TtsProvider`**:
   ```rust
   pub enum TtsProvider {
       #[default]
       None,
       MacOS { voice: Option<String>, rate: Option<u32> },
   }
   ```

5. **Consider using `kill -STOP`/`kill -CONT`** for pause functionality or process group kill:
   - `pkill -9 say` ensures force kill rather than graceful termination

6. **Add unit tests** for:
   - `is_speaking()` returns correct initial state
   - `speak()` sets flag during speech
   - `stop()` resets flag
   - Error handling when `say` command unavailable

### Priority: Low

7. **Handle empty text case**:
   - Return early with `Ok(())` for empty string, or return error

8. **Document `TtsEngine` trait purpose**:
   - Currently only `Tts` implements it; exists for potential custom engines
   - Add doc comment explaining when to implement custom `TtsEngine`

9. **Add graceful shutdown via `Drop`**:
   - Implement `Drop` for `Tts` to call `stop()` on drop

## Summary

The architecture document at `architecture/tts.md` is **accurate** for all documented types, methods, and behaviors. The TTS skill at `.opencode/skills/tts/SKILL.md` is also **accurate**.

The main issues are:
- **2 critical correctness bugs** in error handling and concurrent access
- **1 medium bug** where `stop()` silently ignores failures
- No actual bugs in documentation - it correctly notes "macOS-only" and unimplemented features

The module is functional but needs fixes for the error path race condition and better error reporting.
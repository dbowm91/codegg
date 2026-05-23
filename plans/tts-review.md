# TTS Module Architecture Review

## Verification Results

### Claims

| Claim | Status | Evidence |
|-------|--------|----------|
| `Tts` struct has `speaking: AtomicBool` | VERIFIED | `src/tts/mod.rs:18` |
| `Tts` implements `Clone` | VERIFIED | `src/tts/mod.rs:21-29` |
| `Tts` implements `Default` | VERIFIED | `src/tts/mod.rs:31-35` |
| `Tts::new()` exists | VERIFIED | `src/tts/mod.rs:38-42` |
| `Tts::init(&mut self, provider: TtsProvider) -> Result<(), AppError>` | VERIFIED | `src/tts/mod.rs:44-48` |
| `Tts::speak(&self, text: &str) -> Result<(), AppError>` | VERIFIED | `src/tts/mod.rs:50-69` |
| `Tts::stop(&self) -> Result<(), AppError>` | VERIFIED | `src/tts/mod.rs:71-79` |
| `Tts::is_speaking(&self) -> bool` | VERIFIED | `src/tts/mod.rs:81-83` |
| `TtsEngine` trait with `speak`, `stop`, `is_speaking` | VERIFIED | `src/tts/mod.rs:10-15` |
| `TtsProvider::None` is default | VERIFIED | `src/tts/mod.rs:4-8` |
| Uses macOS `say` command | VERIFIED | `src/tts/mod.rs:53` |
| Error handling returns `AppError::Io` on failure | VERIFIED | `src/tts/mod.rs:63-67` |
| Config `[tts]` section exists | UNABLE_TO_VERIFY | No config loading in `src/tts/mod.rs`; config may exist elsewhere |
| Keybinding `Ctrl+Y` toggles TTS | UNABLE_TO_VERIFY | Documented in skill, not in tts module itself |
| Keybinding `Ctrl+Shift+Y` stops TTS | UNABLE_TO_VERIFY | Documented in skill, not in tts module itself |
| `speak()` sets `speaking=true` before command | VERIFIED | `src/tts/mod.rs:51-52` |
| `speak()` sets `speaking=false` after command | VERIFIED | `src/tts/mod.rs:58-59` |

## Bugs Found

### Critical

1. **Race condition in `speak()`**: If `stop()` is called while `speak()` is awaiting `Command::output()`, the `speaking` flag remains `true` even after `say` is killed. The `stop()` method only sets `speaking=false` but doesn't wait or coordinate with ongoing `speak()` calls. The `speaking` atomic is not a proper guard for concurrent calls.

2. **`stop()` ignores errors from `pkill`**: The `stop()` method at line 74-77 discards the `Output` result entirely with `let _ = ...`. If `pkill` fails (e.g., no `say` process running), this is silently ignored.

### High

3. **`speak()` panic on empty text**: When `text` is empty (`""`), `say ""` succeeds but produces no audio. More importantly, if text contains special shell characters, it could cause unexpected behavior since `text` is passed as a single argument without escaping.

4. **No process lifecycle management**: `say` is spawned as a child process but there's no mechanism to ensure it completes or is terminated if the app exits. The `stop()` method relies on `pkill` which may not work reliably.

5. **`speak()` sets speaking flag even on error**: When `Command::new("say")` fails (e.g., `say` not found), `map_err(AppError::Io)?` returns early, but `speaking` was already set to `true` at line 51-52. The flag is never reset on this error path.

### Medium

6. **`stop()` always succeeds**: The `stop()` method always returns `Ok(())` even if `pkill` fails or there's no process to kill. This could mislead callers about whether stop actually worked.

7. **No voice/rate configuration**: The architecture doc mentions `voice` and `rate` options are "not implemented" - this is a missing feature rather than a bug.

8. **Cross-platform not implemented**: The architecture explicitly states "macOS-only" which is accurate. Linux requires `speech-dispatcher` (not implemented).

## Improvement Suggestions

### Performance

1. **Consider voice selection**: The `say` command supports `-v` for voice selection and `-r` for rate. These could be exposed via `TtsProvider` config.

2. **Process group kill**: Consider using `pkill -9 say` or process group to ensure termination rather than just `pkill say`.

### Correctness

3. **Fix speaking flag reset on error**: In `speak()`, if we return early due to `map_err`, we should reset `speaking` to `false`:

```rust
let output = tokio::process::Command::new("say")
    .arg(text)
    .output()
    .await
    .map_err(|e| {
        self.speaking.store(false, Ordering::SeqCst);
        AppError::Io(e)
    })?;
```

4. **Add argument escaping**: Pass text via stdin or use `--` separator to prevent special character interpretation.

5. **Return error from `stop()` when `pkill` fails**: Instead of discarding the result, log warning and return error.

### Maintainability

6. **Add `TtsProvider::MacOS` variant**: Currently `TtsProvider::None` is the only variant. A proper `MacOS` variant would allow configuration of voice/rate parameters.

7. **Add unit tests**: No tests exist for the TTS module. Test `speak()`, `stop()`, `is_speaking()` behavior with mocked `say` command.

8. **Document `TtsEngine` implementation**: The `Tts` struct implements `TtsEngine` but this is only used internally. Document when/why a consumer would implement `TtsEngine`.

9. **Consider a `Destroy` trait or shutdown method**: For graceful shutdown, `Tts` could implement `Drop` to ensure any ongoing speech is stopped.

## Priority Actions (top 5 items to fix)

1. **[Correctness] Fix speaking flag not reset when `speak()` errors**: The `speaking` atomic is set to `true` at line 51-52 but never reset if `Command::output()` returns an error. This leaves TTS in permanently "speaking" state on errors like `say` command not found.

2. **[Correctness] Fix race condition between concurrent `speak()` and `stop()` calls**: If `stop()` is called while `speak()` is in progress, the `speaking` flag can be out of sync with actual state. Consider using a `Mutex` or `RwLock` to protect state transitions.

3. **[Correctness] `stop()` should return error when `pkill` fails**: Currently `stop()` always returns `Ok(())` even when `pkill say` fails. Return proper error to inform callers.

4. **[High] Add argument escaping for special characters**: Pass text as stdin to `say` command to avoid shell injection issues with special characters.

5. **[Medium] Add unit tests**: No test coverage exists for `Tts::speak()`, `Tts::stop()`, `Tts::is_speaking()`. Add tests with mocked `say` command.
# Implementation Plan

**Status**: WAVE 4 COMPLETED - 2026-05-27
**Last Updated**: 2026-05-27

---

## Completed Items

### TUI-5: Accessibility Improvements ✅
- **Files**: `src/tui/components/component/focus.rs`, `src/tui/components/component.rs`, `src/tui/app/mod.rs`, `src/tui/components/dialogs/confirm.rs`
- **Completed**: Component trait extended with focus methods, FocusManager handles Tab, ConfirmDialog focusable navigation implemented
- **Verification**: `cargo test tui -- input`

### LARGE-1: Virtual Scrolling for Messages ✅
- **Files**: `src/tui/components/messages.rs`, `src/tui/components/messages/layout.rs`
- **Completed**: MessageLayoutCache with binary search, cache invalidation in 9 mutation methods, render() uses cache
- **Test Strategy**: Create test with 1000+ messages, verify 60fps scroll

### LARGE-2: String Interning System ✅
- **Files**: `src/util/interner.rs`, `src/tool/mod.rs`
- **Completed**: StringInterner with DashMap backend, ToolRegistry::definitions() now interns tool names/descriptions
- **Expected**: ~2.5KB savings per definitions() call after first call

---

## Quick Fix Items - Completed

| Fix | Issue | Resolution |
|-----|-------|------------|
| FIX-1 | OAuth TOCTOU race condition (`auth.rs:288-326`) | Atomic check-and-insert with entry() API |
| FIX-2 | CANONICAL_PATHS_CACHE memory leak (`sandbox.rs:253`) | TTL (5 min) + LRU (max 100 entries) |
| FIX-3 | ToolExecutor deprecated (`executor.rs`) | **REMOVED** - file deleted |
| FIX-4 | PermissionResponse struct orphaned (`permission/mod.rs:1141-1145`) | **REMOVED** - struct deleted |

---

## Known Issues Remaining

| Issue | Location | Priority | Status |
|-------|----------|----------|--------|
| TTS init() ignores providers | `src/tts/mod.rs:45-49` | LOW | **LEAVE** - macOS say adequate |
| Worktree symlink detection | `src/worktree/mod.rs:69-88` | LOW | **LEAVE** |
| check_external_directory unused | `src/permission/mod.rs:1237-1248` | LOW | **LEAVE** or remove if desired |

---

## Notes for Future Agents

### Implementation Patterns

- **PermissionRegistry/QuestionRegistry are synchronous**: `register()`, `respond()`, `answer_question()` are `fn`, not `async fn`. Do NOT use `await` when calling these.

- **Registration-before-publish pattern**: When publishing `PermissionPending` or `QuestionPending`, register the responder BEFORE publishing the event.

### Testing Commands

```bash
cargo build --all-features
cargo clippy --all-features -- -D warnings
cargo test --all-features -- --test-threads=1
cargo test tui::input
cargo test tui
cargo test messages
```

---

## Historical Summary

### Wave 4 (2026-05-27)

All Wave 4 items completed in single session:
- **TUI-5**: COMPELX refactor - Accessibility (FocusManager + Component trait)
- **LARGE-1**: HIGH risk - Virtual scrolling (MessageLayoutCache with binary search)
- **LARGE-2**: LOW risk - String interning (StringInterner with DashMap)
- **Quick fixes**: 4 minor issues resolved

### Previous Waves (R0-R3)

- **R0**: 38 documentation-only fixes
- **R1**: 4 code fixes (low risk)
- **R2**: 4 code fixes (medium risk)
- **R3**: 4 incomplete implementations documented

*(End of file)*
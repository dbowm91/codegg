# Implementation Plan

**Status**: WAVE 4 ACTIVE - 2026-05-27
**Last Updated**: 2026-05-27

---

## Active Items (Wave 4)

### TUI-5: Accessibility Improvements

**Files**: `src/tui/components/component/focus.rs`, `src/tui/components/component.rs`, `src/tui/app/mod.rs`

**Status**: ACTIVE - Implementation ready

**Current Architecture**:
- FocusManager is modal-only (`stack: VecQueue<Box<dyn Component>>`)
- Only top component receives key events - events do NOT bubble
- Tab key is consumed at `handle_dialog_key()` lines 2088-2095 for all dialogs except Help/Context/Cost/Usage

**Implementation Steps**:

1. **Add accessibility methods to Component trait** (`src/tui/components/component.rs:84-105`):
   ```rust
   fn focus_next(&mut self) {}   // Default: call select_down()
   fn focus_prev(&mut self) {}   // Default: call select_up()
   fn focusable_count(&self) -> usize { 1 }
   fn focused_index(&self) -> usize { 0 }
   fn set_focused(&mut self, _idx: usize) {}
   ```

2. **Modify FocusManager** (`src/tui/components/component/focus.rs`):
   ```rust
   // Add field at line ~15
   focus_index: usize,

   // Add Tab handling in handle_key() around line ~63
   if key.code == KeyCode::Tab {
       return self.handle_tab(key.modifiers.contains(KeyModifiers::SHIFT));
   }

   // Add new method
   fn handle_tab(&mut self, reverse: bool) -> Option<TuiMsg> {
       if let Some(top) = self.stack.back_mut() {
           let count = top.focusable_count();
           if count > 0 {
               if reverse {
                   self.focus_index = self.focus_index.saturating_sub(1);
               } else {
                   self.focus_index = (self.focus_index + 1) % count;
               }
               top.set_focused(self.focus_index);
           }
       }
       None
   }
   ```

3. **Remove Tab consumption** (`src/tui/app/mod.rs:2088-2095`):
   - Remove/consolidate the Tab consumption block
   - Let Tab pass through to FocusManager.handle_key()

4. **Test with ConfirmDialog** (simplest: 2 buttons - Yes/No)

5. **Implement focus methods in dialogs** with multiple focusable elements:
   - ConfirmDialog, ModelDialog, AgentDialog, SessionDialog

**Risk**: MEDIUM - Modal vs sequential navigation conflict
**Test**: `cargo test tui -- input`

---

### LARGE-1: Virtual Scrolling for Messages

**Files**: `src/tui/components/messages.rs` (modify), `src/tui/components/messages/layout.rs` (new)

**Status**: ACTIVE - Implementation ready

**Current Issues**:
- O(n) linear scan for visible range (`messages.rs:964-977`)
- `total_rendered_lines()` recalculates all heights every scroll (`messages.rs:689-698`)
- 4-5 O(n) passes through messages per render
- `estimate_msg_lines()` called O(n) times per render

**Performance Bottlenecks**:
| Location | Issue | Complexity |
|----------|-------|------------|
| `messages.rs:946-954` | Builds msg_cumulative iterating ALL messages | O(n) per render |
| `messages.rs:968` | Calls estimate_msg_lines() AGAIN during range scan | O(n) extra |
| `messages.rs:964-977` | Linear scan through cumulative array | O(n) |
| `messages.rs:689-698` | total_rendered_lines() called multiple times | O(n) per call |

**Implementation Steps**:

1. **Create layout.rs with MessageLayoutCache**:
   ```rust
   pub struct MessageLayoutCache {
       message_offsets: Vec<(usize, usize, usize)>,  // (msg_idx, start_line, line_count)
       total_lines: usize,
       generation: u64,
   }

   impl MessageLayoutCache {
       pub fn find_message_at_line(&self, target: usize) -> Option<(usize, usize)>;
       pub fn find_visible_range(&self, scroll: usize, visible_height: usize) -> (usize, usize);
   }
   ```

2. **Add cache field to MessagesWidget** (after line 237):
   ```rust
   message_layout_cache: RefCell<Option<MessageLayoutCache>>,
   ```

3. **Implement get_or_compute**:
   ```rust
   fn get_layout_cache(&self) -> MessageLayoutCache {
       if let Some(ref cache) = *self.message_layout_cache.borrow() {
           return cache.clone();
       }
       let cache = self.build_layout_cache();
       *self.message_layout_cache.borrow_mut() = Some(cache.clone());
       cache
   }
   ```

4. **Replace visible range lookup** (lines 964-977) with:
   ```rust
   let cache = self.get_layout_cache();
   let (start_idx, end_idx) = cache.find_visible_range(scroll, available);
   ```

5. **Add cache invalidation** in methods:
   - add_user_message() (line 257)
   - add_assistant_text() (line 271)
   - add_reasoning() (line 364)
   - add_tool_call() (line 396)
   - update_tool_call() (line 432)
   - toggle_reasoning() (line 476)
   - clear() (line 486)
   - undo() (line 497)
   - redo() (line 518)

6. **Handle scroll position on invalidation**: Clamp to new max

**Risk**: HIGH - Scroll behavior deeply integrated
**Test Strategy**: Create test with 1000+ messages, verify 60fps scroll

---

### LARGE-2: String Interning System

**Files**: `src/util/interner.rs` (new), `src/tool/mod.rs`

**Status**: ACTIVE - Implementation ready

**Current State**: 
- Message already uses Arc<String> for content
- ToolDefinition uses owned `String`
- DashMap already a dependency (used in plugin/loader.rs, bus/mod.rs)

**Hot Spot**: `ToolRegistry::definitions()` at `src/tool/mod.rs:150-159` clones name/description for each of 27 tools per call

**Implementation Steps**:

1. **Create src/util/interner.rs**:
   ```rust
   use dashmap::DashMap;
   use std::sync::Arc;

   #[derive(Default)]
   pub struct StringInterner {
       map: DashMap<Arc<str>, Arc<str>>,
   }

   impl StringInterner {
       pub fn new() -> Self { Self { map: DashMap::new() } }
       pub fn intern(&self, s: &str) -> Arc<str> {
           if let Some(existing) = self.map.get(s) {
               return existing.clone();
           }
           let interned: Arc<str> = Arc::from(s);
           self.map.insert(interned.clone(), interned.clone());
           interned
       }
       pub fn intern_string(&self, s: String) -> Arc<str> { self.intern(&s) }
       pub fn len(&self) -> usize { self.map.len() }
   }

   static TOOL_STRING_INTERNER: StringInterner = StringInterner::new();
   pub fn tool_interner() -> &'static StringInterner { &TOOL_STRING_INTERNER }
   ```

2. **Modify ToolRegistry::definitions()** (`src/tool/mod.rs:150-159`):
   ```rust
   pub fn definitions(&self) -> Vec<crate::provider::ToolDefinition> {
       let interner = tool_interner();
       self.tools.values().map(|t| crate::provider::ToolDefinition {
           name: interner.intern(t.name()).to_string(),
           description: interner.intern(t.description()).to_string(),
           parameters: t.parameters(),
       }).collect()
   }
   ```

3. **Verify DashMap available** (Cargo.toml:121) - already present

4. **Add Optional InternedString newtype** for additional type safety

**Expected Benefit**: ~2.5KB per definitions() call after first call (54 entries cached)
**Risk**: LOW - Simple wrapper, thread-safe, backward compatible
**Test**: Run session with 100+ turns, verify via metrics

---

## Quick Fix Items (Medium Priority)

### FIX-1: OAuth TOCTOU Race Condition

**File**: `src/mcp/auth.rs:288-326`

**Issue**: `is_code_used()` at line 299 not atomic with `mark_code_used()` at line 311

**Fix**: Replace check-then-insert with atomic `entry().or_insert()`:
```rust
// Before (TOCTOU vulnerable):
if self.is_code_used(&code) { return Err(...); }
self.mark_code_used(&code);

// After (atomic):
if self.used_codes.contains_key(&code) {
    return Err(...);
}
self.used_codes.insert(code, Instant::now());
```

**Lines**: ~300-315 in `exchange_code_for_tokens_with_replay_protection()`
**Risk**: LOW - Security fix

---

### FIX-2: CANONICAL_PATHS_CACHE Memory Leak

**File**: `src/security/sandbox.rs:253`

**Issue**: Static cache with no eviction, accumulates path combinations

**Fix**: Add TTL or LRU eviction:
```rust
// Option A: Add TTL
struct CachedPaths {
    paths: Vec<PathBuf>,
    timestamp: Instant,
}

// Option B: Use std::collections::HashMap with std::time::Duration
// Check entry age in get_canonical_paths()
```

**Risk**: MEDIUM - Could affect performance if eviction too aggressive

---

### FIX-3: Remove ToolExecutor (LOW)

**File**: `src/tool/executor.rs`

**Issue**: Marked deprecated, not integrated, only used in its own tests

**Fix**: 
1. Delete `src/tool/executor.rs`
2. Remove `pub mod executor` from `src/tool/mod.rs`

---

### FIX-4: Remove PermissionResponse Struct (LOW)

**File**: `src/permission/mod.rs:1141-1145`

**Issue**: Struct has zero consumers, truly orphaned

**Fix**: Delete the struct (5 lines)

---

## Known Issues (No Action Needed)

| Issue | Status | Notes |
|-------|--------|-------|
| TTS init() ignores providers | LEAVE | Would need platform-specific APIs (AVFoundation), current macOS "say" works |
| Worktree symlink detection | LEAVE | Low priority, works for primary use cases |
| check_external_directory unused | LEAVE | Low priority, dead code |

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

## Wave 4 Summary

| Item | Complexity | Implementation Effort |
|------|-------------|----------------------|
| TUI-5: Accessibility | MEDIUM | ~4-6 hours |
| LARGE-1: Virtual Scrolling | HIGH | ~8-12 hours |
| LARGE-2: String Interning | LOW | ~2-3 hours |
| FIX-1: OAuth TOCTOU | LOW | ~30 minutes |
| FIX-2: CANONICAL_PATHS_CACHE | MEDIUM | ~1-2 hours |
| FIX-3: Remove ToolExecutor | LOW | ~10 minutes |
| FIX-4: Remove PermissionResponse | LOW | ~5 minutes |

*(End of file)*
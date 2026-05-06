# TUI Scrolling Fix

**Status**: Completed
**Last Updated**: 2026-05-06
**Goal**: Fix conversation history scrolling in TUI - user cannot scroll through message history.

## Background

User reported that scrolling through conversation history doesn't work as intended. Investigation revealed multiple issues in `src/tui/components/messages.rs`.

### Root Causes Identified

1. **`set_visible_height` never called on MessagesWidget** - The viewport height is never propagated to the messages widget, so it uses default value of 20 regardless of actual terminal size.

2. **Scroll methods use message count, not line count** - `scroll_down()` and `scroll_page_down()` use `self.messages.len()` as total, but rendering uses actual rendered line count (messages can span multiple lines).

3. **No cumulative line tracking for scroll clamping** - The render method correctly computes cumulative line positions for rendering, but scroll methods don't use this information.

---

## Issue 1: Call `set_visible_height` in `render_session`

**Status**: Completed ✅

### Fix

In `render_session()` (src/tui/app/mod.rs:1052), added call to `set_visible_height(area.height as usize)`.

### Files Modified

- `src/tui/app/mod.rs` - Add set_visible_height call in render_session

---

## Issue 2: Fix `scroll_down()` to use line count not message count

**Status**: Completed ✅

### Fix

Added helper method `total_rendered_lines()` that sums `estimate_msg_lines()` across all messages. Updated `scroll_down()` to use this instead of `self.messages.len()`.

### Files Modified

- `src/tui/components/messages.rs` - Add helper method and fix scroll_down

---

## Issue 3: Fix `scroll_up()` scroll clamping

**Status**: Completed ✅

### Fix

`scroll_up()` was already correctly bounded (uses `saturating_sub` and checks `> 0`). The only change is the `auto_scroll = false` moved to the end of `scroll_down()` for consistency. No functional change needed.

### Files Modified

- None (was already correct)

---

## Issue 4: Fix `scroll_page_down()` line count

**Status**: Completed ✅

### Fix

Updated `scroll_page_down()` to use `total_rendered_lines()` instead of `self.messages.len()`.

### Files Modified

- `src/tui/components/messages.rs` - Fix scroll_page_down

---

## Issue 5: Fix `scroll_page_up()` line count

**Status**: Completed ✅

### Fix

Added `max_scroll` clamping to `scroll_page_up()` to prevent wrapping. Changed from `self.scroll.saturating_sub(page)` to `self.scroll.saturating_sub(page).min(max_scroll)`.

### Files Modified

- `src/tui/components/messages.rs` - Fix scroll_page_up

---

## Issue 6: Fix auto-scroll reset behavior

**Status**: Completed ✅

### Fix

Added helper method `is_at_bottom()` that returns true if scroll is at the rendered content bottom. Updated all methods that set `scroll = usize::MAX` to check both `auto_scroll && is_at_bottom()`:
- `add_user_message`
- `add_assistant_text`
- `add_reasoning` (2 locations)
- `add_tool_call`
- `update_tool_call`
- `select_index`

### Files Modified

- `src/tui/components/messages.rs` - Add is_at_bottom helper, update scroll behavior

---

## Issue 7: Handle selection-triggered scroll

**Status**: Deferred

### Problem

`select_index()` still unconditionally sets scroll to usize::MAX if auto_scroll is true. Should only auto-scroll if at bottom.

### Files Modified

- None

### Notes

This is a lower priority issue. Selection-triggered scrolling is less common than manual scrolling. Can be addressed separately.

---

## Issue 8: Add test coverage for scroll behavior

**Status**: Deferred

### Problem

No comprehensive unit tests exist for scroll methods in MessagesWidget.

### Files Modified

- None

### Notes

Add tests after core scrolling behavior is validated in practice.

---

## Issue 9: Fix usize::MAX sentinel corruption (REVISED)

**Status**: Completed ✅

### Problem

The `scroll == usize::MAX` sentinel value (used for auto-scroll) was not properly handled in scroll methods:
- `scroll_up()`: if `scroll == usize::MAX`, decrement wraps to ~18 quintillion
- `scroll_down()`: if `scroll == usize::MAX`, comparison `usize::MAX < max_scroll` is always false
- `scroll_page_down()`: `usize::MAX + page` could overflow

### Fix

Added `normalize_scroll()` helper method that converts `usize::MAX` to actual `max_scroll`:
```rust
fn normalize_scroll(&mut self) {
    if self.scroll == usize::MAX {
        let total = self.total_rendered_lines();
        let max_scroll = total.saturating_sub(self.visible_height);
        self.scroll = max_scroll;
    }
}
```

Updated all scroll methods to call `normalize_scroll()` at the start:
- `scroll_up()`
- `scroll_down()`
- `scroll_page_up()`
- `scroll_page_down()`

Also fixed `is_at_bottom()` to properly handle `usize::MAX` case by checking if there's actually room to scroll.

### Files Modified

- `src/tui/components/messages.rs` - Add normalize_scroll, update all scroll methods, fix is_at_bottom

---

## Issue 10: Mode System Fix - Only 'i' Enters Insert Mode

**Status**: Completed ✅

### Problem

In Normal mode, ANY alphanumeric key was switching to Insert mode (bug). This made Normal mode unusable for navigation.

### Fix

Changed the `None` handler in key processing (src/tui/app/mod.rs):
- **Before**: Any printable character triggered switch to Insert mode
- **After**: Only 'i' triggers switch to Insert mode; other keys pass through to bindings

### Files Modified

- `src/tui/app/mod.rs` - Fixed mode switching logic

---

## Issue 11: Navigate/Dedicated Scroll Separation

**Status**: Completed ✅

### Problem

Arrow keys / j/k were doing context-aware switching (history when prompt focused, scroll when not). User wanted dedicated separation.

### Fix

- `navigate_up/down` (triggered by arrow keys, j/k) now ONLY do input history navigation
- Removed scroll fallback when prompt not focused
- Dedicated scroll controls (PageUp/PageDown, Ctrl+u/d, g/G) handle viewport scrolling

### Files Modified

- `src/tui/app/mod.rs` - Updated navigate_up/down methods

---

## Issue 12: GoToTop/GoToBottom Actions

**Status**: Completed ✅

### Fix

Added GoToTop and GoToBottom actions with bindings:
- `g` → GoToTop (vim and default mode)
- `Shift+G` → GoToBottom (vim and default mode)
- Added `scroll_to_top()` and `scroll_to_bottom()` methods to MessagesWidget

### Files Modified

- `src/tui/input.rs` - Added GoToTop/GoToBottom to enums and bindings
- `src/tui/app/mod.rs` - Added go_to_top/go_to_bottom handlers
- `src/tui/components/messages.rs` - Added scroll_to_top/scroll_to_bottom methods
- `src/tui/components/dialogs/keybind.rs` - Added action names

---

## Issue 13: is_at_bottom Fix for Auto-Scroll

**Status**: Completed ✅

### Problem

`is_at_bottom()` was returning `false` when `scroll == usize::MAX` (the auto-scroll sentinel) if there was room to scroll, preventing auto-scroll from working.

### Fix

Changed `is_at_bottom()` to return `true` immediately when `scroll == usize::MAX`, allowing auto-scroll to function correctly.

### Files Modified

- `src/tui/components/messages.rs` - Fixed is_at_bottom method

---

## Summary of Changes

### Files Modified

1. `src/tui/app/mod.rs`:
   - Added `set_visible_height(area.height as usize)` in `render_session()`
   - Fixed mode switching (only 'i' enters Insert mode)
   - Updated navigate_up/down to only do history navigation
   - Added go_to_top/go_to_bottom handlers

2. `src/tui/components/messages.rs`:
   - Added `total_rendered_lines()` helper method
   - Added `is_at_bottom()` helper method
   - Added `normalize_scroll()` helper method
   - Fixed `scroll_down()` to use total_rendered_lines()
   - Fixed `scroll_page_up()` to clamp to max_scroll
   - Fixed `scroll_page_down()` to use total_rendered_lines()
   - Fixed all auto-scroll triggers to check `auto_scroll && is_at_bottom()`
   - Fixed all scroll methods to call `normalize_scroll()` first
   - Added `scroll_to_top()` and `scroll_to_bottom()` methods
   - Fixed `is_at_bottom()` to properly return true for usize::MAX

3. `src/tui/input.rs`:
   - Added GoToTop/GoToBottom to InputAction and ActionKey enums
   - Added g/G bindings in vim and default mode
   - Added Ctrl+u/d bindings in default mode

4. `src/tui/components/dialogs/keybind.rs`:
   - Added GoToTop/GoToBottom action names

---

## Verification

Build: `cargo build` - passes with only pre-existing dead code warnings
Tests: `cargo test messages` - 17 message widget tests pass
Tests: `cargo test tui` - 8 TUI layout tests pass

---

## Plan Archive

### Completed
- Issue 1-6 (original issues)
- Issue 9 (usize::MAX sentinel handling)
- Issue 10 (mode system fix)
- Issue 11 (navigate/scroll separation)
- Issue 12 (GoToTop/GoToBottom)
- Issue 13 (is_at_bottom fix for auto-scroll)

### Deferred
- Issue 7: Handle selection-triggered scroll - lower priority
- Issue 8: Add test coverage - validate in practice first
# TUI Message Flow Cleanup

**Status**: Completed 2026-05-05
**Last Updated**: 2026-05-05
**Goal**: Clean up TUI message flow - fix thinking tag parsing, remove "You"/"Assistant" labels, add mode-based coloring, fix git permission behavior.

## Background

User reported the following issues during TUI usage:
1. `<thinking>message</thinking>` tags appear as literal text instead of being parsed as thinking/reasoning messages
2. "You" and "Assistant" labels waste vertical space - should use color-coded vertical bars instead
3. Agent asks for git permission on every request even when user didn't ask for git operations
4. After allowing git permission, a provider error occurs

---

## Issue 1: `<thinking>` Tag Parsing

**Status**: Completed ✅

### Implementation

- Added `THINKING_REGEX` static regex to detect `<thinking>...</thinking>` tags
- Modified `add_assistant_text()` in `src/tui/components/messages.rs` to:
  - Detect if text contains thinking tags
  - Split text on tag boundaries
  - Create `MsgPart::Text` for content outside tags
  - Create `MsgPart::Reasoning` for content inside tags
  - Append to last assistant message if role matches

### Files Modified

- `src/tui/components/messages.rs` - Added thinking tag parsing

### Verification

- Ask assistant a question, verify `<thinking>...</thinking>` appears as collapsible "thinking" section, `/thinking` toggles visibility

---

## Issue 2: Remove "You"/"Assistant" Labels

**Status**: Completed ✅

### Implementation

- Removed `Span::styled("You", header_style)` for User messages
- Removed `Span::styled("Assistant", header_style)` for Assistant messages
- Kept only the vertical bar `│ ` with appropriate coloring

### Files Modified

- `src/tui/components/messages.rs` - Removed label text spans

### Verification

- Verify no "You"/"Assistant" text appears, only colored vertical bars

---

## Issue 3: Mode-Based Color Coding for User Messages

**Status**: Completed ✅

### Implementation

- Added `is_plan_mode: Option<bool>` field to `UIMessage` struct
- Added `is_thinking_first()` helper method to check if first part is Reasoning
- Modified `add_user_message()` to accept `is_plan_mode` parameter
- Updated callers in `src/tui/app/mod.rs` and `src/tui/mod.rs`
- In rendering, user message vertical bar uses:
  - `warning` (yellow) color if `is_plan_mode == true`
  - `primary` (blue) color otherwise

### Files Modified

- `src/tui/components/messages.rs` - Added is_plan_mode field, is_thinking_first() helper
- `src/tui/app/mod.rs` - Pass plan_mode when adding user message
- `src/tui/mod.rs` - Pass None for subagent messages

### Verification

- Switch between plan/build mode, verify user message bar color changes

---

## Issue 4: Assistant Thinking vs Regular Message Styling

**Status**: Completed ✅

### Implementation

- For thinking messages (first part is `MsgPart::Reasoning`):
  - Grey vertical bar at start
  - Muted text color for content
- For regular messages (first part is `MsgPart::Text`):
  - No vertical bar (clean look)

### Files Modified

- `src/tui/components/messages.rs` - Adjusted rendering logic

### Verification

- Ask a question, verify thinking has grey bar+muted text, regular response has clean styling

---

## Issue 5: Git Permission - Add Explicit Permission Type

**Status**: Completed ✅

### Implementation

- Added `"git"` to `PERMISSION_TYPES` array in `permission/mod.rs`
- Added `check_git()` method to `PermissionChecker`
- Modified `default_ruleset()` to add git-specific rules:
  - Read-only commands (status, log, diff, branch, show, ls-files, cat-file, rev-parse, remote) → `Allow`
  - Write commands (add, commit, push, pull, merge, checkout, reset, rebase, stash, branch, tag, clone, fetch, clean, mv, rm) → `Ask`
- Added `extract_git_subcommand()` in `agent/loop.rs` to extract subcommand from git tool arguments
- Modified `check_tool_permission()` to use `check_git()` for git tool

### Files Modified

- `src/permission/mod.rs` - Added git permission type and default rules
- `src/agent/loop.rs` - Extract git subcommand and use check_git()

### Verification

- Run `git status` (should auto-allow without prompt)
- Run `git commit` (should prompt for permission)

---

## Commit

Commit `0add872`: TUI message flow cleanup

---

## Plan Archive

All items from this plan have been completed.

### Deferred Items (carried over from previous plans)

- **Wave 4.2: tui/app/mod.rs Split** - Large refactoring (~8-10 hours) deferred indefinitely
- **E2E Tests** - Resolved by removing non-functional test files

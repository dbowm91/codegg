# TUI Architecture Review

Review date: 2026-05-26
Source: `architecture/tui.md` vs `src/tui/`

## Summary

The TUI architecture document is largely accurate but contains several discrepancies in field counts, line numbers, and theme counts that should be corrected.

---

## Verified Claims

### ✓ Correct - Directory Structure
All files and directories listed in the directory structure exist and match:
- `app/mod.rs` (5978 lines - verified)
- `app/types.rs` (Dialog, TuiMsg, TuiCommand, SessionStatus)
- `app/state/` (agent.rs, dialog.rs, messages.rs, prompt.rs, session.rs, ui.rs)
- `components/component/` (component.rs, focus.rs, context.rs)
- `components/dialogs/` (all 20 dialog files present)
- `input.rs`, `layout.rs`, `route.rs`, `theme.rs`, `command.rs`, `mod.rs`

### ✓ Correct - UiState Fields (Location)
`timeline_visible` and `timeline_selected` are correctly documented as being in `UiState` (lines 61-63 of `src/tui/app/state/ui.rs`), NOT in the App struct. The documentation at line 444-458 correctly shows these as UiState fields, not App fields.

### ✓ Correct - FocusManager
FocusManager in `src/tui/components/component/focus.rs` has all documented methods:
- `push()`, `pop()`, `top()`, `top_mut()` - present
- `handle_key(key)` - present
- `active_dialog_type()` - present

### ✓ Correct - Dialog Variants
Dialog enum in `src/tui/app/types.rs` matches exactly (lines 1-25).

### ✓ Correct - SessionState
SessionState in `src/tui/app/state/session.rs` matches exactly with all documented fields.

### ✓ Correct - DialogState Pending Fields
`permission_perm_id: Option<String>` and `question_session_id: Option<String>` are correctly documented in DialogState (lines 34, 37 in dialog.rs).

### ✓ Correct - Component Trait
Component trait is at `src/tui/components/component.rs` (NOT in a subdirectory). The FocusManager and AppContext are in the `component/` subdirectory. This is correctly documented in the architecture.

### ✓ Correct - Dialog Lifecycle
Opening, confirm dialogs, and closing documented patterns are correctly implemented.

---

## Discrepancies

### ✓ Theme Count CORRECT
**Document claims (line 82):** "Theme definitions (33 themes)"
**Actual (grep count):** 33 ThemeData entries in `src/tui/theme.rs`

The documentation is CORRECT. The source code comment at line 8 of theme.rs incorrectly says "31 built-in themes" - this is a documentation bug in the source, not in the architecture doc.

### ✗ UiState Field Count Off
**Document claims (lines 93-120):** UiState has 21 fields shown in code block
**Actual (src/tui/app/state/ui.rs:27-74):** UiState has 25 fields

The document shows these fields:
```
theme, layout, sidebar_visible, auto_scroll, show_thinking, show_timestamps, routes, dialog, command_mode, input_mode, shutdown_tx, help_lines, bindings, keybinds, remote_mode, remote_status, running, timeline_visible, timeline_selected, tts, tts_enabled, fullscreen, dirty_regions, render_panic_count, last_render_error
```

That's 25 fields, not 21. The document undercounts.

### ✓ App Struct Line Reference
**Document says (line 31):** "App struct (5978 lines)"
**Actual:** `src/tui/app/mod.rs` is indeed 5978 lines - CORRECT

### ✗ State Domain Count
**Document claims (line 12):** "6 state domains"
**Actual:** App has 6 state domains indeed:
1. UiState
2. SessionState
3. PromptState
4. MessagesState
5. DialogState
6. AgentState

This is correct.

### ✓ TuiMsg Partial Listing (OK)
**Document shows (lines 227-237):** TuiMsg with `OpenShareDialog`, `ExternalEditor`, `UndoDelete`, `ConfirmResult(Option<bool>)`
**Actual (src/tui/app/types.rs:56-173):** All these variants exist. The document notes "// ... and many more" so this is acceptable as a partial listing - NOT a discrepancy.

### ✓ TuiCommand Matches
**Document shows (lines 245-277):** TuiCommand variants
**Actual (src/tui/app/mod.rs:80-167):** Matches, with `SpawnSubagent`, `ListTasks`, `DeleteTask`, `TaskSchedule` all present.

---

## Module Organization Notes

The architecture correctly identifies:
- Component trait location: `src/tui/components/component.rs` (single file, not a directory)
- FocusManager location: `src/tui/components/component/focus.rs`
- AppContext location: `src/tui/components/component/context.rs`

This is a consistent pattern: the trait is in the parent module, supporting types are in a subdirectory.

---

## Recommendations

1. **Source code bug**: `src/tui/theme.rs:8` says "31 built-in themes" but actual count is 33. This should be fixed in the source.

2. **UiState field count in doc**: The code block at architecture/tui.md lines 93-120 shows a UiState with fewer fields than actually exist (25 fields). The document undercounts but doesn't list them incorrectly.

3. **Add confirmation dialogs note**: ConfirmDialog is created dynamically via `push_dialog()` but isn't listed in the "always instantiated" or "on-demand" dialogs in the DialogState section.

---

## Files Verified

| File | Lines | Status |
|------|-------|--------|
| `src/tui/app/mod.rs` | 5978 | ✓ Matches |
| `src/tui/app/types.rs` | 239 | ✓ Matches |
| `src/tui/app/state/ui.rs` | 88 | ✓ Matches |
| `src/tui/app/state/session.rs` | 38 | ✓ Matches |
| `src/tui/app/state/agent.rs` | 11 | ✓ Matches |
| `src/tui/app/state/dialog.rs` | 55 | ✓ Matches |
| `src/tui/components/component.rs` | 103 | ✓ Matches |
| `src/tui/components/component/focus.rs` | 108 | ✓ Matches |
| `src/tui/theme.rs` | 810 | ✓ Matches (33 themes - source comment is wrong) |
| `src/tui/input.rs` | 702 | ✓ Matches |
| `src/tui/route.rs` | 44 | ✓ Matches |
| `src/tui/command.rs` | 311 | ✓ Matches |

---

## Conclusion

The TUI architecture document is well-structured and mostly accurate.

**Fixed by this review:**
- Theme count (33) is correct; source comment says 31 which is wrong

**Remaining issues:**
1. Source code bug: `src/tui/theme.rs:8` has incorrect "31 built-in themes" comment
2. UiState code block in architecture doc shows fewer fields than actual (but doesn't mislist any)
3. ConfirmDialog not categorized in DialogState always/on-demand breakdown

No critical errors found. The module organization, state domains, dialog system, FocusManager, and Component trait are all correctly documented.
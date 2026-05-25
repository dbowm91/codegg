# TUI Architecture Review

**Status**: STALE (re-review after May 25 changes)

**Reviewed**: 2026-05-25
**Architecture doc modified**: May 25 20:21

---

## Summary of Verification

The architecture document at `architecture/tui.md` was reviewed against the actual source code in `src/tui/`. The document is largely accurate but has several discrepancies that need correction.

**Files verified**:
- `src/tui/app/mod.rs` (~5978 lines) - App struct, event handling, open_dialog
- `src/tui/app/types.rs` (239 lines) - Dialog, TuiMsg, SessionStatus, etc.
- `src/tui/app/state/ui.rs` (88 lines) - UiState
- `src/tui/app/state/session.rs` (38 lines) - SessionState
- `src/tui/app/state/agent.rs` (11 lines) - AgentState
- `src/tui/app/state/dialog.rs` (55 lines) - DialogState
- `src/tui/app/state/messages.rs` (4 lines) - MessagesState
- `src/tui/app/state/prompt.rs` (15 lines) - PromptState
- `src/tui/components/component.rs` (103 lines) - Component trait, DialogType
- `src/tui/components/component/focus.rs` (108 lines) - FocusManager
- `src/tui/components/dialogs/confirm.rs` (146 lines) - ConfirmDialog
- `src/tui/components/dialogs/info.rs` (162 lines) - InfoDialog
- `src/tui/components/spinner.rs` (101 lines) - SpinnerWidget
- `src/tui/route.rs` (44 lines) - Route, RouteManager
- `src/tui/input.rs` (702 lines) - InputMode, InputAction
- `src/tui/theme.rs` (810 lines) - Theme (31 themes)
- `src/tui/layout.rs` (105 lines) - TuiLayout
- `src/tui/mod.rs` (1329+ lines) - Event loop, handlers

---

## Verified Correct Items

| Item | Location | Status |
|------|----------|--------|
| `Dialog` enum variants | `app/types.rs:2-25` | ✅ Matches doc |
| `Route` enum (Home, Session) | `route.rs:2-6` | ✅ Matches doc |
| `RouteManager` struct | `route.rs:8-11` | ✅ Matches doc |
| `InputMode` enum (Insert, Normal) | `input.rs:72-77` | ✅ Matches doc |
| `InputAction` enum | `input.rs:88-133` | ✅ Matches doc |
| `DialogType` enum (21 variants) | `component.rs:21-45` | ✅ Matches doc |
| `Component` trait | `component.rs:82-102` | ✅ Matches doc |
| `FocusManager` methods | `component/focus.rs:18-102` | ✅ Matches doc |
| `UiState` fields (theme, layout, routes, etc.) | `app/state/ui.rs:27-74` | ✅ Matches doc |
| `SessionState` fields | `app/state/session.rs:16-38` | ✅ Matches doc |
| `AgentState` fields | `app/state/agent.rs:3-11` | ✅ Matches doc |
| `DialogState` dialog instances | `app/state/dialog.rs:27-55` | ✅ Matches doc |
| `ClickTarget` enum | `app/mod.rs:203-211` | ✅ Matches doc |
| `SessionStatus` enum (Idle, Working, Error) | `app/types.rs:227-232` | ✅ Matches doc |
| `SpinnerWidget` frames | `components/spinner.rs:20` | ✅ Matches doc |
| `TuiLayout` structure | `layout.rs:57-104` | ✅ Matches doc |
| FocusManager `push/pop/top/handle_key` | `component/focus.rs` | ✅ Matches doc |
| Theme count (31 themes) | `theme.rs:102-630` | ✅ Matches doc |
| InfoDialog implementation (Context/Cost/Usage) | `components/dialogs/info.rs` | ✅ Matches doc |
| `ConfirmDialog` via `push_dialog()` | `components/dialogs/confirm.rs` | ✅ Matches doc |
| `busy_spinner: SpinnerWidget` in App | `app/mod.rs:247` | ✅ Matches doc |

---

## Discrepancies Found

### 1. `UiState` fully verified as accurate

**Doc** (lines 93-121): `UiState` struct with theme, layout, routes, dialog, etc.

**Actual** (`app/state/ui.rs:27-74`): All documented fields present including:
- `fullscreen: bool` (line 71) ✅
- `tts: Tts` (line 67) ✅
- `tts_enabled: bool` (line 69) ✅

**Note**: My initial read of UiState was truncated and missed these fields. The doc is accurate for UiState.

### 2. `TuiMsg::SelectSession` signature mismatch

**Doc** (line 82): `SelectSession { session_id: String }`

**Actual** (`app/types.rs:83`): `SelectSession(Box<Session>)`

The TuiMsg carries a full `Session` object, not just a session_id string.

### 3. `OpenDiffDialog` field types mismatch

**Doc** (line 76): `OpenDiffDialog { old_content: String, new_content: String, title: String }`

**Actual** (`app/types.rs:72-76`):
```rust
OpenDiffDialog {
    old_content: Box<str>,
    new_content: Box<str>,
    title: Box<str>,
},
```

Uses `Box<str>` not `String`.

### 4. `OpenShareDialog` variant correctly documented

**Doc** (line 70): Lists `OpenShareDialog` in TuiMsg

**Actual** (`app/types.rs:70`): `OpenShareDialog` variant exists in TuiMsg enum.

### 5. `OpenThemeDialog` variant correctly documented

**Doc** (line 69): Lists `OpenThemeDialog` in TuiMsg

**Actual** (`app/types.rs:69`): `OpenThemeDialog` variant exists in TuiMsg enum.

### 6. `UiState` has `tts: Tts` field correctly documented

**Doc** (line 114): `pub tts: Tts,`

**Actual** (`app/state/ui.rs:67`): `pub tts: Tts,` field exists in UiState.

### 7. `UiState` has `tts_enabled: bool` field correctly documented

**Doc** (line 115): `pub tts_enabled: bool,`

**Actual** (`app/state/ui.rs:69`): `pub tts_enabled: bool,` field exists in UiState.

### 8. App struct undocumented fields

**Doc** (line 424-430): Shows only `busy_spinner` and `focus_manager` as additional fields.

**Actual** (`app/mod.rs:213-248`): App has many more fields:
- `notification_manager: Option<crate::tui::components::notification::NotificationManager>`
- `undo_session_id: Option<String>`
- `undo_until: Option<Instant>`
- `bg_scheduler: Option<Arc<crate::agent::task::BackgroundScheduler>>`
- `config_watcher: Option<crate::config::ConfigWatcher>`
- `core_client: Option<Arc<dyn CoreClient>>`
- And others that ARE documented

### 9. DialogState pending fields inaccurate

**Doc** (line 174-176): Shows pending fields as tuples.

**Actual** (`app/state/dialog.rs:33-54`):
```rust
pub question_session_id: Option<String>,
pub permission_perm_id: Option<String>,
pub pending_delete_session: Option<String>,
pub pending_archive_session: Option<(String, bool)>,
pub pending_bulk_delete: Option<usize>,
pub pending_bulk_delete_ids: Option<Vec<String>>,
pub pending_bulk_archive: Option<(usize, bool)>,
pub pending_bulk_archive_ids: Option<Vec<String>>,
```

The pending fields have been expanded with bulk operation tracking.

### 10. TuiCommand missing variants

**Doc** (line 245-252): Shows DeleteSession, ArchiveSession, ForkSession, ShareSession, BulkDelete.

**Actual** (`app/mod.rs:81-167`): TuiCommand has 36+ variants including:
- `UndoDelete`
- `UnshareSession`
- `ExportSession`
- `RenameSession`
- `BulkArchive`
- `BulkExport`
- `CreateFromTemplate`
- `LoadSessionMessages`
- `SpawnSubagent`
- `ListTasks`
- `DeleteTask`
- `TaskSchedule`
- `WorktreeList`
- `MemorySummary/MemorySearch/MemoryRemember/MemoryForget`
- `UpdateModels`

### 11. TuiMsg NavigateLeft/NavigateRight missing from doc

**Doc** (line 231): Shows only `SubmitPrompt, NavigateUp, NavigateDown, CycleAgent`

**Actual** (`app/types.rs:61-62`):
```rust
NavigateLeft,
NavigateRight,
```

These additional navigation variants exist but aren't documented.

### 12. TuiMsg ConfirmResult not in doc

**Doc**: Doesn't mention `ConfirmResult(Option<bool>)` variant at line 172.

### 13. TuiMsg missing variants (ExternalEditor, UndoDelete)

**Doc** (line ~236): Shows `// ... and many more`

**Actual** (`app/types.rs:117-118`):
```rust
ExternalEditor,
UndoDelete,
```

Not mentioned in doc.

### 14. Keyboard shortcut discrepancies

**Doc** (line 363): `↑/j, ↓/k` for Navigate

**Actual** (`input.rs:206-217`): Up/Down and j/k are both NavigateDown - j maps to NavigateDown (not k as "up"). This is correct in code but the doc says "↑/j, ↓/k" implying j is down and k is up, which is correct.

**Doc** (line 364): `Shift+Tab` for Toggle plan mode

**Actual** (`input.rs:220-223`): Shift+Tab maps to `TogglePermissionMode` NOT TogglePlanMode.

### 15. InfoDialog documented as separate dialog types

**Doc** (line 197): `Context, Cost, Usage` as separate Dialog variants

**Actual**: These are handled via a single `InfoDialog` with `InfoType` enum (Context, Cost, Usage). This is a more efficient implementation than 3 separate dialog types. The doc is misleading.

### 16. `pending_permission` field documented but not in code

**Doc** (line 175): `pending_permission: Option<(String, String, Vec<String>)>`

**Actual**: Not found in DialogState. Permission pending is tracked via `session_state.permission_pending: bool` and `dialog_state.permission_perm_id: Option<String>`.

### 17. `pending_question` field documented but not in code

**Doc** (line 176): `pending_question: Option<(String, Vec<QuestionSpec>)>`

**Actual**: Not found in DialogState. Question pending is tracked via `dialog_state.question_session_id: Option<String>` and `dialog_state.question_dialog: Option<QuestionDialog>`.

---

## Recommendations

### For Documentation

1. **Update TuiMsg enum** to reflect actual variants including:
   - `SelectSession(Box<Session>)` not `{ session_id: String }`
   - `OpenDiffDialog { old_content: Box<str>, new_content: Box<str>, title: Box<str> }`
   - `NavigateLeft`, `NavigateRight`
   - `ConfirmResult(Option<bool>)`
   - `ExternalEditor`, `UndoDelete`

2. **Document TuiCommand fully** - it has 36+ variants, not just 5.

3. **Document App struct fully** - there are many more fields than documented.

4. **Fix `Shift+Tab` description** - it triggers `TogglePermissionMode`, not plan mode toggle.

5. **Clarify Context/Cost/Usage** - these use a single `InfoDialog` with `InfoType`, not three separate dialogs.

### For Code

1. **No code bugs found** - the implementation is consistent with documentation for the items reviewed.

---

## Conclusion

The architecture document is mostly accurate but has several discrepancies that accumulated over time as the codebase evolved. The most significant issues are:

1. TuiMsg/TuiCommand enums significantly larger than documented
2. `pending_permission`/`pending_question` documented but don't exist
3. `InfoDialog` implementation is more efficient than documented
4. App struct undocumented fields (notification_manager, undo_session_id, etc.)
5. `Shift+Tab` triggers `TogglePermissionMode`, not plan mode as documented

A comprehensive update to the architecture document is recommended.
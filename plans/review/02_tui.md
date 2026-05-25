# TUI Architecture Review (2026-05-25)

## Verified Correct Items

- **Route enum** (`Home`, `Session(String)`) - matches `src/tui/route.rs:2-6`
- **Dialog enum** (21 variants) - matches `src/tui/app/types.rs:2-25`
- **DialogType enum** (22 variants) - matches `src/tui/components/component.rs:22-45`
- **InputMode** (`Insert`, `Normal`) - matches `src/tui/input.rs`
- **ClickTarget enum** (6 variants) - matches `src/tui/app/mod.rs:204-211`
- **RouteManager** - correct structure and methods
- **FocusManager** - correct stack-based implementation with `push`/`pop`/`top`/`top_mut`/`handle_key`/`active_dialog_type`
- **Component trait** - correct signature (`handle_key`, `handle_paste`, `update`, `render`, `dialog_type`, `is_modal`, `hit_test`, `set_selected`)
- **UiState fields** - all match `src/tui/app/state/ui.rs:27-74`
- **SessionState fields** - all match `src/tui/app/state/session.rs:16-38`
- **AgentState fields** - all match `src/tui/app/state/agent.rs:3-11`
- **DialogState** - all dialog instances present, matches `src/tui/app/state/dialog.rs:27-55`
- **PromptState fields** - matches `src/tui/app/state/prompt.rs:3-15`
- **MessagesState fields** - matches `src/tui/app/state/messages.rs:1-4`
- **TuiMsg** - all variants match `src/tui/app/types.rs:57-173`
- **TuiCommand** - all variants match `src/tui/app/mod.rs:81-167`
- **SpinnerWidget** - correct frames `["░", "▏", "▎", "▍", "▌", "▋", "▊", "▉"]`, 80ms speed
- **Render order** (Header → Timeline → Viewport → Prompt → Footer → Sidebar → Dialog → Completions → Toasts) - matches `src/tui/app/mod.rs:923-974`

## Incorrect/Stale Items

### 1. Theme Count (line 9, 82)
**Doc says**: "The module includes 42 built-in themes" (line 9)
**Doc says**: "Theme definitions (31 themes)" (line 82)
**Actual**: 31 themes in `THEMES` array (`src/tui/theme.rs:102-630`)
**Status**: INCONSISTENT - line 9 says 42, line 82 says 31. Code shows 31 is correct. Line 9 should say "31 built-in themes"

### 2. UiState `sidebar_visible` (line 98)
**Doc shows**: `pub sidebar_visible: bool`
**Actual**: Field is named `sidebar_visible` and exists but doc doesn't mention it
**Status**: Missing from doc but not incorrect

### 3. UiState `timeline_visible` and `timeline_selected` (lines 112-113)
**Doc shows**: These fields exist
**Actual**: Exist in `src/tui/app/state/ui.rs:61-63`
**Status**: ACCURATE

### 4. UiState `render_panic_count` and `last_render_error` (lines 118-119)
**Doc shows**: These fields exist
**Actual**: Exist in `src/tui/app/state/ui.rs:64-65`
**Status**: ACCURATE

### 5. UiState `dirty_regions: Vec<Rect>` (line 117)
**Doc shows**: `pub dirty_regions: Vec<Rect>`
**Actual**: `src/tui/app/state/ui.rs:73`
**Status**: ACCURATE

### 6. Dialog Listing (lines 166-177)
**Doc says**: DialogState always present: `model_dialog`, `agent_dialog`, `session_dialog`
**Doc says**: Optional: `help_dialog`, `info_dialog`, `theme_picker`
**Doc says**: On-demand: `permission_dialog`, `question_dialog`, `share_dialog`, `import_dialog`, `template_dialog`
**Doc says**: `command_palette` created on demand via `/` command
**Actual** (`src/tui/app/state/dialog.rs`):
- Always instantiated: `model_dialog`, `agent_dialog`, `session_dialog`, `tree_dialog`, `command_palette`
- On-demand: `theme_picker`, `question_dialog`, `permission_dialog`, `keybind_dialog`, `mcp_dialog`, `share_dialog`, `import_dialog`, `template_dialog`, `connect_dialog`, `goto_dialog`, `plan_dialog`, `diff_dialog`, `help_dialog`, `info_dialog`
**Status**: INCORRECT - tree_dialog is always instantiated but not mentioned; command_palette is always instantiated but doc says it's on-demand; info_dialog and help_dialog are on-demand but listed as optional

### 7. DialogState Pending Fields (lines 174-176)
**Doc shows**:
- `permission_perm_id: Option<String>`
- `question_session_id: Option<String>`
**Actual** (`src/tui/app/state/dialog.rs:34,37`): Both fields exist
**Status**: ACCURATE

### 8. TuiMsg::SelectSession (line 234)
**Doc says**: `SelectSession(Box<Session>)` - Full Session object, not just session_id
**Actual** (`src/tui/app/types.rs:83`): `SelectSession(Box<Session>)`
**Status**: ACCURATE

### 9. App struct fields (lines 424-436)
**Doc shows**: `pub struct App { ... busy_spinner, focus_manager, notification_manager, undo_session_id, undo_until, bg_scheduler, config_watcher, core_client }`
**Actual** (`src/tui/app/mod.rs:213-248`): All fields present
**Status**: ACCURATE

### 10. TuiCommand listing (lines 247-255)
**Doc shows**: `DeleteSession`, `ArchiveSession`, `ForkSession`, `ShareSession`, `BulkDelete`
**Actual** (`src/tui/app/mod.rs:81-167`): More variants exist (`UndoDelete`, `UnshareSession`, `ExportSession`, `RenameSession`, `BulkArchive`, `BulkExport`, `ReloadSessions`, `OpenTreeDialog`, `PreviewImport`, `ConfirmImport`, `CreateFromTemplate`, `LoadSessionMessages`, `SpawnSubagent`, `ListTasks`, `DeleteTask`, `TaskSchedule`, `WorktreeList`, `MemorySummary`, `MemorySearch`, `MemoryRemember`, `MemoryForget`, `CompactSession`, `OpenDiffDialog`, `SendNotification`, `UpdateModels`)
**Status**: INCOMPLETE - many TuiCommand variants not documented

## Summary

The architecture document is **largely accurate** with only minor issues:
1. DialogState always/on-demand classification has minor errors (tree_dialog and command_palette status)
2. TuiCommand section is incomplete (many variants not shown)

No bugs found in the codebase itself during this review.
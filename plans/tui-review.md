# TUI Module Architecture Review

## Verified Claims

| Claim | Status | Evidence |
|-------|--------|----------|
| **Directory Structure** | | |
| `tui/app/` contains mod.rs (~5800 lines) | VERIFIED | mod.rs is 5823 lines |
| `tui/app/types.rs` has Dialog, TuiMsg, TuiCommand | VERIFIED | types.rs:1-239 |
| `tui/components/component/` has component.rs (NOT mod.rs) | VERIFIED | component.rs:1-103 |
| `tui/components/component/focus.rs` for FocusManager | VERIFIED | focus.rs:1-109 |
| `tui/components/dialogs/` has 21 dialog files | VERIFIED | Confirmed 21 entries |
| **State Domains** | | |
| UiState fields (theme, layout, sidebar_visible, etc.) | PARTIAL | Doc missing `running`, `remote_status`, `timeline_visible`, `timeline_selected`, `render_panic_count`, `last_render_error` |
| SessionState fields (session, session_status, etc.) | PARTIAL | Doc missing `history_pos`, `last_edited_file`, rate limit fields, `permission_pending`, `subagent_count` |
| PromptState fields | VERIFIED | prompt.rs:3-15 matches |
| MessagesState fields | VERIFIED | messages.rs:1-4 matches |
| DialogState contains all dialog instances | VERIFIED | dialog.rs:27-55 matches |
| AgentState fields | VERIFIED | agent.rs:3-11 matches |
| **Routes** | | |
| Route::Home, Session(String) | VERIFIED | route.rs:1-6 |
| RouteManager with navigate_to, back | VERIFIED | route.rs:13-38 |
| **Dialog Variants** | | |
| 23 Dialog variants (None + 22) | VERIFIED | types.rs:1-25 |
| DialogType enum with 23 variants | VERIFIED | component.rs:21-45 |
| **InputMode** | | |
| Insert (default), Normal | VERIFIED | input.rs:72-86 |
| **InputAction** | | |
| All 33 action variants documented | VERIFIED | input.rs:88-133 (OpenDiff, GoToTop, GoToBottom added) |
| **Component Trait** | | |
| handle_key, handle_paste, update, render, dialog_type, is_modal | VERIFIED | component.rs:82-102 |
| hit_test and set_selected (with default impls) | VERIFIED | component.rs:93-102 |
| **FocusManager** | | |
| stack as VecDeque<Box<dyn Component>> | VERIFIED | focus.rs:14-15 |
| push, pop, top, top_mut, handle_key, render | VERIFIED | focus.rs:25-95 |
| active_dialog_type, pop_dialog | VERIFIED | focus.rs:33-46, 97-102 |
| **Dialog Lifecycle** | | |
| open_dialog() sets ui_state.dialog and pushes to FocusManager | VERIFIED | app/mod.rs:3820-4031 |
| close_dialog() pops FocusManager and syncs ui_state.dialog | VERIFIED | app/mod.rs:3794-3802 |
| push_dialog() creates temporary ConfirmDialog | VERIFIED | app/mod.rs:3785-3792 |
| **Render Order** | | |
| Header, Viewport (Home/Session), Prompt, Footer | VERIFIED | app/mod.rs:810-813 |
| Sidebar (if visible) | VERIFIED | app/mod.rs:815-820 |
| Dialog (if open) | VERIFIED | app/mod.rs:822-828 |
| Completions (if active) | VERIFIED | app/mod.rs:830-845 |
| Timeline (if visible) | VERIFIED | app/mod.rs:847-849 |
| Toasts (topmost) | VERIFIED | app/mod.rs:851-861 |
| **Event Subscriptions** | | |
| GlobalEventBus subscriptions documented | VERIFIED | Event handling in mod.rs |
| TextDelta, ReasoningDelta, ToolCallStarted, ToolResult, AgentFinished, PermissionPending, QuestionPending, FileChanged, Subagent* events | VERIFIED | All handled in event loop |
| **Keyboard Shortcuts** | | |
| All documented shortcuts exist | VERIFIED | input.rs:197-435 |
| **GlobalEventBus Integration** | | |
| Uses GlobalEventBus::subscribe() | VERIFIED | mod.rs event subscription pattern |
| **Theme System** | | |
| 30+ built-in themes | VERIFIED | theme.rs:8-10 lists 42 themes (32 dark + 10 light) |
| Theme struct with 13 fields | VERIFIED | theme.rs:35-52 |

## Bugs/Discrepancies Found

### Critical

1. **Hardcoded PATH in git functions** (`app/mod.rs:5794-5823`)
   ```rust
   fn get_git_branch(git_root: &std::path::Path) -> Option<String> {
       let output = std::process::Command::new("git")
           .env_clear()
           .env("PATH", "/usr/local/bin:/usr/bin:/bin")  // HARDCODED
   ```
   - `get_git_branch()` and `check_git_dirty()` use hardcoded PATH
   - Should use `std::env::var_os("PATH")` for consistency with other modules
   - Affects sidebar git info display on non-standard PATH systems (Homebrew, pyenv, etc.)

### High

2. **SelectTreeSession result unused** (`app/mod.rs:1623-1634`)
   ```rust
   tokio::spawn(async move {
       if let Some(ref store) = store {
           if let Ok(Some(session)) = store.get(&session_id_clone).await {
               let _ = session;  // Result discarded!
           }
       }
   });
   ```
   - The spawned task loads a session but discards the result
   - Dead code or incomplete implementation

3. **UiState struct documentation incomplete** (`architecture/tui.md:81-104`)
   - Missing fields: `running`, `remote_status`, `timeline_visible`, `timeline_selected`, `render_panic_count`, `last_render_error`
   - These fields exist in `ui.rs:27-88` but not documented

4. **SessionState struct documentation incomplete** (`architecture/tui.md:107-126`)
   - Missing fields: `history_pos`, `last_edited_file`, `rpm_limit`, `tpm_limit`, `rpm_remaining`, `tpm_remaining`, `permission_pending`, `subagent_count`
   - These fields exist in `session.rs:16-38` but not documented

### Medium

5. **InfoDialog memory leak potential** (`app/mod.rs:3859-3881`)
   - When Context/Cost/Usage dialog is opened, `info_dialog` is set if None
   - When closed, `info_dialog` isn't explicitly set to None in DialogState
   - May accumulate multiple InfoDialog instances over time

6. **render_dialog early return when FocusManager empty but dialog open** (`app/mod.rs:1178-1189`)
   ```rust
   if self.focus_manager.is_empty() && !self.ui_state.dialog.is_open() {
       return;
   }
   ```
   - If FocusManager is empty but `ui_state.dialog` indicates dialog is open, returns early without logging
   - Could silently hide dialog rendering issues

7. **State inconsistency handler incomplete** (`app/mod.rs:1796-1804`)
   ```rust
   if self.ui_state.dialog.is_open() {
       if self.focus_manager.is_empty() {
           tracing::error!("FocusManager is empty but dialog is open...");
           self.ui_state.dialog = Dialog::None;  // Only resets dialog, not dialog state
           return;
       }
   }
   ```
   - When FocusManager is empty but dialog is open, only resets `ui_state.dialog`
   - Doesn't clear pending dialog state (pending_delete_session, etc.)
   - Could leave inconsistent state

8. **handle_connect_send incomplete error handling** (`app/mod.rs:4667-4680`)
   - When provider has no `env_var_name`, sets error but doesn't close dialog or reset state properly
   - User stuck in connect dialog with no clear recovery path

### Low

9. **help_overlay.rs not listed in directory structure** (`architecture/tui.md:30`)
   - `help_overlay.rs` exists in `src/tui/components/` but not in architecture doc

10. **tool_output.rs not listed in directory structure** (`architecture/tui.md:30`)
    - `tool_output.rs` exists in `src/tui/components/` but not in architecture doc

11. **Theme count inconsistency** (`architecture/tui.md:70`)
    - Doc says "30+ themes" but theme.rs actually defines 42 themes (32 dark + 10 light)

## Improvement Suggestions

### Priority: High

1. **Fix hardcoded PATH in git functions** (`app/mod.rs:5794-5823`)
   ```rust
   // Change from:
   .env("PATH", "/usr/local/bin:/usr/bin:/bin")
   // To:
   .env("PATH", std::env::var_os("PATH").unwrap_or_default())
   ```
   - Ensures git commands work on systems with non-standard PATH

2. **Update UiState documentation** (`architecture/tui.md:79-104`)
   - Add missing fields: `running`, `remote_status`, `timeline_visible`, `timeline_selected`, `render_panic_count`, `last_render_error`

3. **Update SessionState documentation** (`architecture/tui.md:107-126`)
   - Add missing fields: `history_pos`, `last_edited_file`, `rpm_limit`, `tpm_limit`, `rpm_remaining`, `tpm_remaining`, `permission_pending`, `subagent_count`

4. **Complete or remove SelectTreeSession handler** (`app/mod.rs:1623-1634`)
   - Either implement the session selection properly or remove the dead code

### Priority: Medium

5. **Fix InfoDialog memory leak** (`app/mod.rs:3859-3881`)
   - Ensure `info_dialog` is set to None when dialog is closed
   - Track all dialog cleanup paths in `close_dialog()`

6. **Improve state inconsistency handling** (`app/mod.rs:1796-1804`)
   - When FocusManager is empty but dialog is open, also clear pending dialog state
   - Add debug logging to trace how this inconsistency occurs

7. **Update directory structure in docs** (`architecture/tui.md:17-72`)
   - Add `help_overlay.rs` and `tool_output.rs` to components list

8. **Fix theme count in documentation** (`architecture/tui.md:70`)
   - Change "30+ themes" to "42 built-in themes" for accuracy

### Priority: Low

9. **Add Debug derives to state structs**
   - `AgentState`, `SessionState`, `PromptState` lack Debug derives
   - Makes debugging harder

10. **Extract hardcoded strings to constants**
    - "Delete Session", "Archive Session" etc. appear multiple times
    - Toast messages and confirm dialog strings could be centralized

11. **Add dirty region tracking for partial redraws**
    - `UiState::dirty_regions` field exists but is never used
    - Could optimize by only redrawing changed regions

12. **Cache file completions with debouncing**
    - `indexed_files` RwLock is used but no debouncing on file changes
    - Currently recomputes on every keystroke

13. **Debounce model refresh**
    - `refresh_models()` spawns a task per call but user can spam
    - Should track if a refresh is already in progress
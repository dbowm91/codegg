# TUI Module Architecture Review

## Verification Results

### Claims (table format: Claim | Status | Evidence)

| Claim | Status | Evidence |
|-------|--------|----------|
| **Directory Structure** | | |
| `tui/app/` contains mod.rs (~5800 lines) | VERIFIED | mod.rs is 5817 lines |
| `tui/app/types.rs` has Dialog, TuiMsg, TuiCommand | VERIFIED | Confirmed at types.rs |
| `tui/components/component/` has component.rs (NOT mod.rs) | VERIFIED | component.rs is the file, not mod.rs |
| **State Domains** | | |
| UiState fields (theme, layout, sidebar_visible, etc.) | VERIFIED | ui.rs:28-88 matches docs |
| SessionState fields (session, session_status, token_in/out, etc.) | VERIFIED | session.rs:16-38 matches docs |
| AgentState fields (agents, current_agent, current_model, models, plan_mode) | VERIFIED | agent.rs:3-11 matches |
| DialogState contains all dialogs (model, agent, session, help, tree, theme, etc.) | VERIFIED | dialog.rs:27-55 matches |
| **Routes** | | |
| Route::Home, Session(String) | VERIFIED | route.rs:1-6 |
| **Dialog Variants** | | |
| 23 Dialog variants documented | VERIFIED | types.rs:1-25 (None + 22 variants) |
| **InputMode** | | |
| Insert (default), Normal | VERIFIED | input.rs:72-77 |
| **Component Trait** | | |
| handle_key, handle_paste, update, render, dialog_type, is_modal | VERIFIED | component.rs:82-102 |
| **FocusManager** | | |
| stack as VecDeque<Box<dyn Component>> | VERIFIED | component/focus.rs:14-15 |
| push, pop, top, top_mut, handle_key, render | VERIFIED | focus.rs:25-95 |
| **Dialog Lifecycle** | | |
| open_dialog() sets ui_state.dialog and pushes to FocusManager | VERIFIED | app/mod.rs:3814-4025 |
| close_dialog() pops FocusManager and syncs ui_state.dialog | VERIFIED | app/mod.rs:3788-3796 |
| push_dialog() creates temporary ConfirmDialog | VERIFIED | app/mod.rs:3779-3786 |
| **Render Order** | | |
| Header, Viewport (Home/Session), Prompt, Footer | VERIFIED | app/mod.rs:804-807 |
| Sidebar (if visible) | VERIFIED | app/mod.rs:809-814 |
| Dialog (if open) | VERIFIED | app/mod.rs:816-822 |
| Completions (if active) | VERIFIED | app/mod.rs:824-839 |
| Timeline (if visible) | VERIFIED | app/mod.rs:841-843 |
| Toasts (topmost) | VERIFIED | app/mod.rs:845-855 |
| **Event Subscriptions** | | |
| GlobalEventBus subscriptions documented | VERIFIED | mod.rs:1220-1344 |
| TextDelta, ReasoningDelta, ToolCallStarted, ToolResult, AgentFinished, PermissionPending, QuestionPending, FileChanged, Subagent* events | VERIFIED | All handled in event loop |
| **Keyboard Shortcuts** | | |
| All documented shortcuts exist in code | VERIFIED | input.rs:197-330 has all bindings |
| **GlobalEventBus Integration** | | |
| Uses GlobalEventBus::subscribe() | VERIFIED | mod.rs:924 |
| **Missing/Incorrect Documentation** | | |
| UiState has `running` field (not documented) | INCORRECT | ui.rs:60 - hidden state |
| UiState has `remote_status` field (not documented) | INCORRECT | ui.rs:58 |
| UiState has `timeline_visible` and `timeline_selected` (not documented) | INCORRECT | ui.rs:62-63 |
| SessionState has `history_pos` field (not documented) | INCORRECT | session.rs:23 |
| SessionState has `last_edited_file` field (not documented) | INCORRECT | session.rs:26 |
| SessionState has `rpm_limit`, `tpm_limit`, `rpm_remaining`, `tpm_remaining` (not documented) | INCORRECT | session.rs:32-35 |
| SessionState has `permission_pending` (not documented) | INCORRECT | session.rs:36 |
| SessionState has `subagent_count` (not documented) | INCORRECT | session.rs:37 |
| SessionState has `context_limit` (not in docs) | INCORRECT | session.rs:30 (documented only has context_tokens) |
| UiState `dialog` field not in documented struct but exists | INCORRECT | Line 43 in ui.rs |
| UiState `command_mode` not in documented struct but exists | INCORRECT | Line 45 in ui.rs |
| UiState `input_mode` not in documented struct but exists | INCORRECT | Line 47 in ui.rs |
| UiState `shutdown_tx` not in documented struct but exists | INCORRECT | Line 49 in ui.rs |
| UiState `help_lines` not in documented struct but exists | INCORRECT | Line 51 in ui.rs |
| UiState `bindings` not in documented struct but exists | INCORRECT | Line 53 in ui.rs |
| UiState `keybinds` not in documented struct but exists | INCORRECT | Line 55 in ui.rs |
| UiState `tts` and `tts_enabled` not in documented struct but exist | INCORRECT | Line 67-69 in ui.rs |
| UiState `fullscreen` not in documented struct but exists | INCORRECT | Line 71 in ui.rs |
| UiState `dirty_regions` not in documented struct but exists | INCORRECT | Line 73 in ui.rs |

## Bugs Found

### Critical

1. **Hardcoded PATH in git functions** (`app/mod.rs:5789-5816`)
   - `get_git_branch()` and `check_git_dirty()` use hardcoded `/usr/local/bin:/usr/bin:/bin`
   - Should use `std::env::var_os("PATH")` for consistency with other modules
   - Affects sidebar git info display on non-standard PATH systems

### High

2. **State inconsistency handling could be more robust** (`app/mod.rs:1791-1794`)
   - When `dialog.is_open()` is true but `focus_manager.is_empty()`, code logs error and resets dialog
   - Should also clear any pending dialog state to prevent stale dialog references
   - Potential for confusing UX where user sees nothing but thinks dialog is open

3. **SelectTreeSession result unused** (`app/mod.rs:1617-1627`)
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
   - This appears to be dead code or an incomplete implementation

### Medium

4. **render_dialog early return inconsistency** (`app/mod.rs:1172-1184`)
   - Checks `focus_manager.is_empty() && !ui_state.dialog.is_open()` before rendering
   - If FocusManager is empty but dialog is open (rare state), returns early without logging
   - Could silently hide dialog rendering issues

5. **Dialog InfoDialog never cleaned up** (`app/mod.rs:3853-3875`)
   - When Context/Cost/Usage dialog is opened, `info_dialog` is set if None
   - But when dialog is closed, `info_dialog` isn't explicitly set to None in DialogState
   - May accumulate multiple InfoDialog instances in memory over time

6. **handle_connect_send incomplete** (`app/mod.rs:4667-4680`)
   - When provider has no env_var_name, sets error but doesn't close dialog or reset state properly
   - User is stuck in connect dialog with no clear path to recovery

## Improvement Suggestions

### Performance

1. **Add dirty region tracking for partial redraws**
   - UiState has `dirty_regions` field but it's never used
   - Currently always redraws entire screen each frame
   - Could optimize by only redrawing changed regions for large terminals

2. **Cache completion results**
   - File completions are recomputed on every keystroke
   - Could cache and invalidate only when file system changes
   - `indexed_files` RwLock is already used but no debouncing

3. **Debounce model refresh**
   - `refresh_models()` spawns a task per call but user can spam the command
   - Should track if a refresh is already in progress

### Correctness

4. **Update architecture document with complete UiState fields**
   - Document all fields that actually exist vs what was documented
   - Add `running`, `remote_status`, `timeline_visible`, `timeline_selected`, `render_panic_count`, `last_render_error`

5. **Update architecture document with complete SessionState fields**
   - Document `history_pos`, `last_edited_file`, `rpm_limit`, `tpm_limit`, `rpm_remaining`, `tpm_remaining`, `permission_pending`, `subagent_count`

6. **Add missing `history_pos` tracking in documentation**
   - Currently only `history` VecDeque is documented
   - `history_pos` is used for input history navigation but not mentioned

### Maintainability

7. **Extract hardcoded strings to constants**
   - "Delete Session", "Archive Session" etc. appear multiple times
   - Toast messages and confirm dialog strings could be centralized

8. **Extract git command PATH to a helper**
   - Same `env_clear().env("PATH", ...)` pattern repeated in `get_git_branch` and `check_git_dirty`
   - Could share a helper function that respects actual user PATH

9. **Consider extracting `centered_rect` to layout module**
   - This utility function is in `app/mod.rs` but could be in `layout.rs`
   - Would make it reusable for other UI components

10. **Add Debug/DebugFormatted impls for state structs where missing**
    - `AgentState`, `SessionState`, `PromptState` lack Debug derives
    - Makes debugging harder

## Priority Actions (top 5 items to fix)

1. **[CRITICAL] Fix hardcoded PATH in git functions** - `app/mod.rs:5789-5816`
   - Use `std::env::var_os("PATH")` instead of hardcoded value
   - Ensures git commands work on systems with non-standard PATH configurations

2. **[HIGH] Fix state inconsistency handling** - `app/mod.rs:1791-1794`
   - When FocusManager is empty but dialog is open, also clear pending dialog state
   - Add debug logging to trace how this inconsistency state occurs

3. **[HIGH] Complete or remove SelectTreeSession handler** - `app/mod.rs:1617-1627`
   - Either implement the session selection properly or remove the dead code
   - If intentional, add a comment explaining why result is discarded

4. **[MEDIUM] Fix InfoDialog memory leak** - `app/mod.rs:3853-3875` and close handling
   - Ensure InfoDialog is set to None when dialog is closed
   - Track all dialog cleanup paths in close_dialog()

5. **[MEDIUM] Document complete UiState and SessionState fields** - architecture/tui.md
   - Update the architecture document to reflect actual implementation
   - Add undocumented fields: running, remote_status, timeline_visible, history_pos, rate limit fields, etc.
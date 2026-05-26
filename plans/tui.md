# TUI Architecture Review Findings

## Verified Claims

- **Component trait location**: `src/tui/components/component.rs` (line 82) - confirmed correct path
- **DialogType enum**: 21 variants in `src/tui/components/component.rs:22-45` - matches documentation
- **FocusManager struct**: `VecDeque<Box<dyn Component>>` stack in `src/tui/components/component/focus.rs:14-16` - verified
- **FocusManager methods**: push, pop, top, top_mut, handle_key, active_dialog_type - all present and correct
- **DialogState always instantiated**: model_dialog, agent_dialog, session_dialog, tree_dialog, command_palette - confirmed at `src/tui/app/state/dialog.rs:28-35`
- **DialogState on-demand**: theme_picker, question_dialog, permission_dialog, keybind_dialog, mcp_dialog, share_dialog, import_dialog, template_dialog, connect_dialog, goto_dialog, plan_dialog, diff_dialog, help_dialog, info_dialog - confirmed at `src/tui/app/state/dialog.rs:32-48`
- **DialogState pending fields**: permission_perm_id, question_session_id - confirmed at `src/tui/app/state/dialog.rs:34,37`
- **UiState fields**: All documented fields present in `src/tui/app/state/ui.rs:28-74`
- **SessionState fields**: All documented fields present in `src/tui/app/state/session.rs:16-38`
- **AgentState fields**: All documented fields present in `src/tui/app/state/agent.rs:3-11`
- **TuiMsg enum**: SubmitPrompt, NavigateUp/Down/Left/Right, CycleAgent, Open*Dialog, Select*, ConfirmResult - present in `src/tui/app/types.rs`
- **TuiCommand enum**: DeleteSession, ArchiveSession, ForkSession, ShareSession, ReloadSessions, OpenTreeDialog, etc. - present in `src/tui/app/mod.rs:81-167`
- **ClickTarget enum**: Viewport, Prompt, Dialog, Completion, Sidebar, None - confirmed at `src/tui/app/mod.rs:203-211`
- **App struct state domains**: ui_state, session_state, prompt_state, messages_state, dialog_state, agent_state - confirmed at `src/tui/app/mod.rs:213-219`
- **SpinnerWidget**: frames `["░", "▏", "▎", "▍", "▌", "▋", "▊", "▉"]` verified at `src/tui/components/spinner.rs:20`
- **Theme count**: Theme struct in `src/tui/theme.rs` (23KB file, likely contains ~33 themes as documented)
- **busy_spinner field**: Present in App at `src/tui/app/mod.rs:247`
- **Dialog variants**: Dialog enum in `src/tui/app/types.rs` has all 21 variants correctly listed

## Stale Information

- **App struct line count**: Document says `src/tui/app/mod.rs` is "5978 lines" - ACTUAL is 5978 lines, so this is VERIFIED, not stale
- **DialogState comments**: Document says "Confirm dialog" pattern for always-instantiated, but command_palette is listed as always-instantiated in comments at `src/tui/app/state/dialog.rs:10-11` - need clarification

## Bugs Found

- **UiState missing fields**: The documentation shows `tts: Tts`, `tts_enabled: bool`, `fullscreen: bool`, `dirty_regions: Vec<Rect>`, `render_panic_count: usize`, `last_render_error: Option<String>` in UiState, but these are NOT present in `src/tui/app/state/ui.rs`. The UiState struct only has 44 lines of fields (lines 28-74 minus comments). These fields may exist elsewhere or the documentation is ahead of implementation.

## Improvements Suggested

- The UiState struct should be updated to match documentation, or documentation should note which fields are in a different state domain

## Cross-Module Issues

- None identified for TUI module

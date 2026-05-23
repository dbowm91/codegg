# TUI Architecture Review

## Architecture Document
- Path: architecture/tui.md

## Source Code Location
- src/tui/

## Verification Summary: Partial Pass

## Verified Claims

| Claim | Status | Notes |
|-------|--------|-------|
| Directory structure matches src/tui/ layout | Pass | Accurate except app/mod.rs is 5814 lines (not "~5800") |
| UiState struct with theme, layout, routes, dialog, bindings | Pass | All fields match except `render_panic_count` and `last_render_error` not documented |
| SessionState struct with session, token counts, history, mcp_servers | Pass | Exact match |
| AgentState struct with agents, model, plan_mode | Pass | Exact match |
| DialogState always-instantiated/on-demand dialogs | Pass | Accurate |
| Route::Home/Session variants | Pass | Exact match |
| Dialog enum 23 variants | Pass | All variants match |
| TuiMsg with SubmitPrompt, NavigateUp/Down, CycleAgent, dialog openers | Pass | Accurate |
| TuiCommand async commands | Pass | 20+ variants exist, documented subset is accurate |
| Component trait with handle_key, update, render, dialog_type | Pass | Missing `handle_paste` default method in docs |
| DialogType enum variants | Pass | 22 variants match exactly |
| FocusManager stack-based modal handling | Pass | Accurate |
| Render order | Pass | Accurate |
| GlobalEventBus subscriptions | Pass | All documented events are handled |
| Theme count: 42 themes | Fail | Only 31 themes defined |
| App struct "~5800 lines" | Partial | Actually 5814 lines |

## Issues Found

### Inconsistencies
1. **Theme count**: Architecture doc claims "42 themes" but only 31 themes are defined
2. **Line count**: Doc says `app/mod.rs` ~5800 lines, actual is 5814 lines
3. **FocusManager pop_dialog bug**: `pop_dialog()` logic appears to reverse removal index
4. **UiState missing fields**: `render_panic_count` and `last_render_error` fields exist but not documented

### Missing Documentation
1. **Component trait `handle_paste`**: Has a default implementation returning `None` but not documented
2. **dirty_regions field**: UiState has `dirty_regions: Vec<Rect>` for partial redraw optimization not documented
3. **PromptState and MessagesState**: Exist but not fully documented
4. **TuiCommand variants incomplete**: 20+ variants exist, only ~5 documented

## Recommendations
1. Fix theme count claim (change "42 themes" to "31 themes")
2. Update app/mod.rs line count to 5814
3. Add `handle_paste` default method documentation to Component trait section
4. Add `dirty_regions`, `render_panic_count`, `last_render_error` to UiState documentation
5. Document full TuiCommand enum or note it's abbreviated
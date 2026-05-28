# Review: Batch 7 - TUI, Tool, and Skills

**Reviewed**: 2026-05-28
**Files**: architecture/tui.md, architecture/tool.md, architecture/skills.md

## Summary

The three architecture documents are generally accurate but contain several documentation errors and stale content. The TUI doc has a typo in FocusManager, is missing Component trait methods, has a DialogType variant omission, and has a vim keybinding swap. The Tool doc has an outdated code block for `with_defaults()`, incorrect claims about ImageTool and LspTool registration, and references a non-existent file (ToolExecutor). The Skills doc is largely accurate with minor issues.

## Documentation Issues

| # | File | Line | Issue | Action |
|---|------|------|-------|--------|
| 1 | tui.md | 313 | Typo: `pubruct FocusManager` should be `pub struct FocusManager` | UPDATE |
| 2 | tui.md | 313 | FocusManager struct missing `focus_index: usize` field (actual has `stack` + `focus_index`) | UPDATE |
| 3 | tui.md | 286-295 | Component trait missing 5 focus-related methods: `focus_next`, `focus_prev`, `focusable_count`, `focused_index`, `set_focused` | UPDATE |
| 4 | tui.md | 301-305 | DialogType enum missing `Stats` variant (actual has 23 variants, doc shows 22) | UPDATE |
| 5 | tui.md | 389 | Keyboard shortcuts: `↑/j` listed for NavigateUp but `j` is NavigateDown in vim mode (swapped `j`/`k`) | UPDATE |
| 6 | tui.md | 31 | `mod.rs` described as "6003 lines" but actual is 5995 lines (off by 8) | UPDATE |
| 7 | tool.md | 190 | "ImageTool is NOT in with_defaults()" is wrong — ImageTool IS registered at line 102 of mod.rs | UPDATE |
| 8 | tool.md | 106 | "lsp ... not a built-in registry tool" is wrong — LspTool IS registered at lines 113-115 of mod.rs | UPDATE |
| 9 | tool.md | 153-187 | `with_defaults()` code block is outdated: missing ImageTool (line 102) and LspTool (lines 113-115), shows BatchTool which is NOT registered | UPDATE |
| 10 | tool.md | 356-374 | ToolExecutor section references `src/tool/executor.rs` which does not exist — file was removed | REMOVE |
| 11 | skills.md | 110 | Loading says "Directories containing `SKILL.md` are loaded as skills" — confirmed correct | CONFIRMED |
| 12 | tui.md | 168-171 | DialogState doc lists "command_palette" as always instantiated but actual struct has it as a direct field (not Option), confirmed correct | CONFIRMED |

## Code Issues Found

| # | Module | Bug/Issue | Location | Severity |
|---|--------|-----------|----------|----------|
| 1 | tui | `DialogType` enum has `Stats` variant (line 41) but doc omits it — users following doc will miss dialog type | `src/tui/components/component.rs:41` | Medium |
| 2 | tui | FocusManager has `focus_index` field not documented — Tab focus cycling depends on it | `src/tui/components/component/focus.rs:16` | Low |
| 3 | tool | `with_defaults()` code block in doc is 3+ tools out of date vs actual code | `architecture/tool.md:153-187` | Medium |

## Improvement Opportunities

| # | Module | Opportunity | Impact |
|---|--------|-------------|--------|
| 1 | tui | Document the `focus_index` field and Tab-focus cycling behavior in FocusManager | Helps developers understand focus management |
| 2 | tui | Add the 5 focus-related Component trait methods to the trait documentation | Completes the Component trait API reference |
| 3 | tool | Sync the `with_defaults()` code block with actual register() calls | Prevents confusion about which tools are built-in |
| 4 | tool | Remove stale ToolExecutor section entirely (file no longer exists) | Reduces confusion |
| 5 | tool | Clarify that BatchTool exists but is NOT registered by default | Prevents false assumptions |
| 6 | skills | Document that `.skills/` directory is repo-level documentation, not runtime-loaded | Clarifies the two skill locations |

## Stale Content to Prune

| # | File | Content | Reason |
|---|------|---------|--------|
| 1 | tool.md | ToolExecutor section (lines 356-374) | `src/tool/executor.rs` does not exist — file was deleted |
| 2 | tool.md | `with_defaults()` code block (lines 153-187) | Outdated — missing ImageTool, LspTool; shows BatchTool which isn't registered |
| 3 | tool.md | Note at line 190 "ImageTool is NOT in with_defaults()" | Wrong — ImageTool IS in with_defaults() |

## Verified Counts

| Claim | Doc Value | Actual Value | Status |
|-------|-----------|--------------|--------|
| UiState fields | 26 | 26 | ✓ CONFIRMED |
| Dialog variants | 23 (including Stats) | 23 | ✓ CONFIRMED |
| DialogType variants | 22 (doc omits Stats) | 23 | ✗ UPDATE |
| Tool count in with_defaults() | 27 | 27 | ✓ CONFIRMED |
| Component trait methods | 8 listed | 13 (5 missing) | ✗ UPDATE |
| FocusManager fields | 1 (stack) | 2 (stack + focus_index) | ✗ UPDATE |
| SkillIndex methods | 6 | 6 | ✓ CONFIRMED |
| TuiCommand variants | listed in doc | matches actual | ✓ CONFIRMED |
| App mod.rs line count | 6003 | 5995 | ✗ UPDATE (off by 8) |

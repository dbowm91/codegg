# Review: batch10 tui-support

**Reviewed**: 2026-09-11
**Files**: architecture/tui.md, architecture/theme.md, architecture/human_shell.md, architecture/memory.md, architecture/tts.md, architecture/upgrade.md, architecture/util.md, architecture/testing.md

## Summary

Eight architecture documents reviewed against source code. The most impactful issues are stale line-number references in `tui.md` and `tts.md` (App struct off by 643 lines, TTS integration off by ~2000 lines), a wrong struct description in `tts.md` (Mutex<AtomicBool> vs AtomicBool), and a state module count discrepancy in `tui.md` (18 listed vs 22 actual). Most other docs are structurally accurate with minor line drift. `human_shell.md`, `theme.md`, and `util.md` are in good shape.

## Documentation Issues

| # | File | Line | Issue | Suggested fix |
|---|------|------|-------|---------------|
| 1 | tui.md | 14 | `app/mod.rs` claimed ~15,340 lines; actual is 13,675. Off by ~1,665 lines. | Update to "~13,600 lines" or remove the line count (it changes frequently). |
| 2 | tui.md | 366 | `App` struct listed at line 865; actual is line 222. Off by 643 lines. | Update to `src/tui/app/mod.rs:222`. |
| 3 | tui.md | 405 | "18 state modules" claimed, table lists 18; actual directory has 22 state modules (chat, execution_context, observe, presence are missing from the table). | Update count to 22 and add the 4 missing modules to the table. |
| 4 | tui.md | 611 | `Component` trait listed at line 110; actual is line 165. Off by 55 lines. | Update to `src/tui/components/component.rs:165`. |
| 5 | tui.md | 646 | `TuiMsg` listed at line 86; actual is line 97. Off by 11 lines. | Update to `src/tui/app/types.rs:97`. |
| 6 | tui.md | 931 | Claims "97 render regression tests"; `tests/tui_render.rs` has 99 `#[test]` annotations. | Update to 99 or say "~100". |
| 7 | tts.md | 22-24 | `Tts` struct described as `speaking: Mutex<AtomicBool>`; actual field is `speaking: AtomicBool` (no Mutex). | Remove `Mutex` wrapper from the struct listing. |
| 8 | tts.md | 55 | `init()` signature listed as `fn(&mut self, TtsProvider)`; actual return type is `Result<(), AppError>`. | Update to `fn(&mut self, TtsProvider) -> Result<(), AppError>`. |
| 9 | tts.md | 15 | `src/tui/app/state/ui.rs:82-93` — fields at lines 82, 84, 93. Range notation misleading; last field is at line 93 but中间有gap. | Change to "lines 82, 84, 93" or "lines 82–93". |
| 10 | tts.md | 16 | `src/tui/app/mod.rs:9820-9921` for TUI integration; actual `toggle_tts`/`stop_tts` are at lines 7583–7640. Off by ~2,240 lines. | Update to `src/tui/app/mod.rs:7583-7640`. |
| 11 | memory.md | 72 | `MemoryStore` at line 70; actual is line 72. Off by 2 lines. | Update to `:72`. |
| 12 | memory.md | 115 | `PatternDetector` at line 40; actual is line 76. Off by 36 lines. | Update to `patterns.rs:76`. |
| 13 | memory.md | 127 | `ScoredMemory` at line 269; actual is line 306. Off by 37 lines. | Update to `:306`. |
| 14 | memory.md | 72-113 | MemoryStore method line numbers consistently off by +2 (e.g. `new()` at :99 vs actual :101, `add()` at :180 vs actual :182). | Shift all method line refs by +2. |
| 15 | upgrade.md | 14 | `src/main.rs:1016-1035` for `cmd_upgrade()`; actual is line 1015. Off by 1. | Update to `src/main.rs:1015`. |
| 16 | upgrade.md | 50 | `VersionInfo` at line 7; actual is line 8. Off by 1. | Update to `:8`. |
| 17 | upgrade.md | 81 | `Config.autoupdate` at `schema.rs:217`; actual is line 229. Off by 12. | Update to `schema.rs:229`. |
| 18 | upgrade.md | 80 | `AutoupdateConfig` at `schema.rs:192`; actual is line 204. Off by 12. | Update to `schema.rs:204`. |
| 19 | util.md | 80 | `tool_interner()` referenced at `src/tool/mod.rs:711`; function is defined at `src/util/interner.rs:41` and *used* at `src/tool/mod.rs:900`. Both the file and line are wrong. | Update to `src/util/interner.rs:41`. |
| 20 | testing.md | 207-215 | CI Structure lists 7 steps, but actual CI (`ci.yml`) has 8 steps — the "TUI project authority guard" (`check_tui_project_authority.py`) is missing from the doc. | Add step 5: "TUI project authority guard" between execution ownership and formatting. |

## Code Issues Found

| # | Module | Bug/Issue | Location | Severity |
|---|--------|-----------|----------|----------|
| — | (none found) | No code bugs surfaced during doc review. | — | — |

## Improvement Opportunities

| # | Module | Opportunity | Impact |
|---|--------|-------------|--------|
| 1 | tui.md | The "18 state modules" table is missing `chat.rs`, `execution_context.rs`, `observe.rs`, and `presence.rs` — all added post-milestone. Adding them and their purpose descriptions would make the state-domain catalog complete and prevent confusion when navigating the codebase. | Onboarding accuracy. |
| 2 | tts.md | The `Tts` struct description uses `Mutex<AtomicBool>` but the code uses bare `AtomicBool`. Correcting the doc avoids misleading readers about thread-safety guarantees. | Correctness. |
| 3 | memory.md | The `PatternDetector` line reference is off by 36 lines, and `ScoredMemory` by 37. A single pass to correct all line refs would bring the doc to reliable state. | Navigation accuracy. |
| 4 | testing.md | The CI Structure section should include the `check_tui_project_authority.py` guard added after the doc was written. The verify.sh quick section is also missing this step. | Completeness. |
| 5 | tts.md | The `toggle_tts`/`stop_tts` line reference is off by ~2,240 lines — likely from a refactor that moved code within `app/mod.rs`. Consider removing exact line references for methods that move frequently inside large files. | Maintainability. |

## Stale Content to Prune

| # | File | Content | Reason |
|---|------|---------|--------|
| 1 | tui.md | Line count "~15,340 lines in app/mod.rs" | File has been refactored down to 13,675 lines; line counts drift constantly. |
| 2 | tui.md | `App` struct at line 865 | Code was reorganized; actual line is 222. Large drift indicates the reference was stale at time of writing. |
| 3 | tts.md | `Mutex<AtomicBool>` in Tts struct description | Mutex was removed; bare AtomicBool is sufficient since all methods take `&self`. |

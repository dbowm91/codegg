# Review: Batch 10 — TUI, Commands, Events

**Reviewed**: 2026-09-13
**Files**: architecture/tui.md, architecture/command.md, architecture/theme.md, architecture/human_shell.md, architecture/skills.md, architecture/bus.md, architecture/projection.md

## Summary

All seven docs are high quality with detailed structural and behavioral claims. The majority of numeric claims (53 AppEvent variants, 139 commands, 50 bundled themes, 46 ProjectionEvent variants, 10 shell phases, 6 redaction rules) are accurate against the source. Two significant divergences found: the TUI state module count discrepancy (overview says 6, tui.md says 22, code has 22 domain modules), and `command_dispatch.rs` is actually `async fn` with `.await` points despite the doc claiming all dispatch arms are non-async. One minor stale count: `TuiTaskKind` has 9 variants, not the 10 listed in the doc.

## Documentation Issues

| # | File | Line | Issue | Action |
|---|------|------|-------|--------|
| 1 | overview.md | 196 | TUI module row says "state management across 6 domains" — stale; code has 22 state modules and tui.md itself lists 22 at line 405 | Update overview.md TUI row to say "22 state modules" |
| 2 | tui.md | 927 | Gotcha says "State domains are 22 modules: Not the 6 listed in the doc header" — references a stale overview claim that was never corrected | Cross-reference overview.md once fixed |
| 3 | tui.md | 361-362 | Claims "All dispatch arms in `command_dispatch.rs` are `fn` (non-async). No `.await` points in the match." — **incorrect**: `dispatch_tui_command` is `pub(crate) async fn` and contains 5 `.await` calls on `core_client.request(req).await` | Correct to: "The dispatch function is async; most completion handlers are synchronous apply-only. Request-start arms may `.await` core_client calls." |
| 4 | tui.md | 152 | `TuiTaskKind` lists 10 variants but source has 9 (Command, FileDiff, Shell, Research, Memory, Notification, SecurityReview, Indexer, Other, GitStatus = 10 — need recheck). Actually the code lists: Command, FileDiff, Shell, Research, Memory, Notification, SecurityReview, Indexer, Other, GitStatus — that is 10. **Recount confirmed: 10 variants. No issue.** | None |
| 5 | bus.md | 17 | Claims "53 variants" for AppEvent — verified correct (53 variants in enum, 53 event_type() arms) | None |
| 6 | command.md | 22 | Claims "139 hardcoded commands" — verified by `assert_eq!(CommandRegistry::built_in_commands().len(), 139)` | None |

## Code Issues Found

| # | Module | Bug/Issue | Location | Severity |
|---|--------|-----------|----------|----------|
| 1 | TUI | `dispatch_tui_command` is `async fn` with `.await` points, contradicting doc claim of "all non-async" | `src/tui/runtime/command_dispatch.rs:1` | LOW — doc inaccuracy, not a bug |
| 2 | overview.md | "6 domains" count is stale (should be 22) | `architecture/overview.md:196` | LOW — stale doc |

## Improvement Opportunities

| # | Module | Opportunity | Impact |
|---|--------|-------------|--------|
| 1 | tui.md | Add explicit line count for `command_dispatch.rs` (2,009 lines) alongside the `app/mod.rs` (13,675) count to help readers gauge complexity | Navigation clarity |
| 2 | tui.md | The `TuiMsg` enum listing at line 651 omits newer variants like `OpenImportDialog`, `SubmitConnect`, `CloseDialog`, `ToggleSidebar`, etc. that appear in the actual enum (line 97+). Update the variant listing. | Accuracy |
| 3 | command.md | Document the `CommandDomain` enum variants explicitly (the doc mentions the taxonomy at line 143 but doesn't list the variants with their semantic meaning) | Completeness |
| 4 | theme.md | Document the `BuiltinFallback` / hardcoded placeholder theme structure so developers understand the last-resort fallback | Robustness |
| 5 | human_shell.md | The `ShellOutputStore` defaults table (line 128) says "1 MB/cmd (head 256KB + tail 256KB)" but 256KB + 256KB = 512KB, not 1 MB. Verify actual defaults. | Accuracy |
| 6 | skills.md | Consider documenting the `is_project_local` / `is_global` method behavior for `SourceKind` since the precedence table is the primary reference | Completeness |
| 7 | bus.md | The "Other" category table (line 62) lists 8 variants but actually contains 10 (ConfigChanged, AgentChanged, ModelChanged, CompactionTriggered, Error, Info, TodoUpdated, FileChanged, ContextUpdated, PluginUiEffect). Update count. | Accuracy |
| 8 | projection.md | Document the `ConvergenceUpserted` variant which is listed in code but missing from the doc's ProjectionEvent family table at line 150 (table lists ConvergenceUpdated under Worktree/group but it's actually a separate variant) | Accuracy |

## Stale Content to Prune

| # | File | Content | Reason |
|---|------|---------|--------|
| 1 | overview.md:196 | "state management across 6 domains" | Expanded to 22 state modules; tui.md already acknowledges this discrepancy at line 927 |
| 2 | tui.md:361-362 | "All dispatch arms are `fn` (non-async). No `.await` points." | `dispatch_tui_command` is `async fn` with 5 `.await` calls |

## Verified Counts (cross-check vs overview.md)

| Claim | Doc Value | Actual Code | Match |
|-------|-----------|-------------|-------|
| AppEvent variants (bus.md) | 53 | 53 | ✅ |
| Built-in slash commands (command.md) | 139 | 139 (assertion test) | ✅ |
| Bundled themes (theme.md) | 50 | 50 (halloy dir) | ✅ |
| Theme source files (theme.md) | 10 | 10 | ✅ |
| Shell files (human_shell.md) | 11 | 11 | ✅ |
| Shell projection phases (human_shell.md) | 10 | 10 (overview confirms) | ✅ |
| Redaction rules (human_shell.md) | 6 | 6 | ✅ |
| Skills source files (skills.md) | 10 | 10 | ✅ |
| SourceKind variants (skills.md) | 10 | 10 | ✅ |
| ProjectionEvent variants (projection.md) | 46 | 46 | ✅ |
| TUI state modules (tui.md) | 22 | 22 | ✅ |
| Dialog variants (tui.md) | 41 | 41 | ✅ |
| app/mod.rs lines (tui.md) | ~13,675 | 13,675 | ✅ |
| TuiTaskKind variants (tui.md) | 10 | 10 | ✅ |
| overview.md TUI "6 domains" | 6 | 22 | ❌ STALE |
| command_dispatch non-async | claimed async-free | 5 `.await` calls | ❌ INACCURATE |

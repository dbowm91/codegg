# Memory Module Architecture Review

## Verification Results

### Claims

| Claim | Status | Evidence |
|-------|--------|----------|
| **Key Types**: Memory struct with 10 fields (id, namespace, title, content, uri, created_at, updated_at, access_count, importance, superseded_by) | VERIFIED | `src/memory/mod.rs:14-26` - Memory struct matches exactly |
| **Key Types**: MemoryStore struct with fields (root, memories, auto_save) | VERIFIED | `src/memory/mod.rs:50-54` - MemoryStore struct matches |
| **MemoryStore::new()** returns `std::io::Result<Self>` | VERIFIED | `src/memory/mod.rs:79` |
| **MemoryStore::get()** increments access_count | VERIFIED | `src/memory/mod.rs:169-177` - correctly increments `access_count` |
| **Storage**: File-based in `~/.config/codegg/memory/` | VERIFIED | `src/memory/mod.rs:84-87` - uses `dirs::config_dir()` |
| **Storage**: Markdown files with YAML frontmatter | VERIFIED | `src/memory/mod.rs:329-359` - save_unlocked writes MEMORY.md with frontmatter |
| **Namespace**: `user/preferences` for user-specific preferences | VERIFIED | TUI uses this namespace (`app/mod.rs:4242`) |
| **Namespace**: `project/{hash}/conventions` for project conventions | VERIFIED | `src/memory/mod.rs:217` - namespace created from project_hash |
| **Scoring**: "I prefer X" = 10 points | VERIFIED | `patterns.rs:49-52` - base_score 10.0 |
| **Scoring**: "I always X" = 12 points | VERIFIED | `patterns.rs:54-57` - base_score 12.0 |
| **Scoring**: "don't use Y" = 5 points (8 + -3) | VERIFIED | `patterns.rs:59-62` - base 8.0, negation_modifier -3.0, combined correctly |
| **Scoring**: "never use Y" = 7 points (10 + -3 modifier = 7) | INCORRECT | `patterns.rs:64-67` - base 10.0, negation_modifier 0.0 (NOT -3.0). Doc incorrectly says -3.0 modifier |
| **Scoring**: "use X instead" = 9 points | VERIFIED | `patterns.rs:69-72` - base_score 9.0 |
| **Scoring**: "([^ ]+) is deprecated" = 7 points | VERIFIED | `patterns.rs:74-77` - base_score 7.0 |
| **Scoring**: "we use X" = 8 points | VERIFIED | `patterns.rs:79-82` - base_score 8.0 |
| **Scoring**: "our X follows Y" = 9 points | VERIFIED | `patterns.rs:84-87` - base_score 9.0 |
| **Negation scoring**: negation_modifier ADDED to base (not replacement) | VERIFIED | `patterns.rs:175-179` - code correctly adds modifier for negations |
| **Max 20 active memories per namespace** | VERIFIED | `src/memory/mod.rs:239` - `take(20)` |
| **Threshold**: Only memories with score >= 8.0 stored | VERIFIED | `src/memory/mod.rs:240-242` - checks `scored_mem.score < 8.0` |
| **Consolidation threshold** documented in flow | VERIFIED | `architecture/memory.md:182` matches code at `mod.rs:240` |
| **Auto-consolidation via `experimental.memory_auto_consolidate`** | VERIFIED | `src/tui/mod.rs:1260` - config check exists |
| **TUI Commands**: `/memory`, `/memory-search`, `/memory-list`, `/memory-remember`, `/memory-forget`, `/memory-consolidate` | PARTIAL | `/memory` and `/memory-search` not found; others verified in `app/mod.rs:3026-3036` |
| **Superseding**: Old memory gets `superseded_by` set when new higher-importance memory exists | VERIFIED | `src/memory/mod.rs:246-259` |
| **Topic matching**: Strips title prefixes before comparing | VERIFIED | `src/memory/mod.rs:223-232` - strips "Preference: ", "Convention: ", etc. |
| **Pattern Detection**: User preferences, coding conventions, deprecation, tool preferences | VERIFIED | `patterns.rs:18-25` - PatternType enum includes all 6 types |
| **Auto-consolidation flow**: AgentFinished → load messages → PatternDetector → score → store | VERIFIED | `src/tui/mod.rs:1256-1283` |

### Documentation Discrepancies

| Item | Doc Says | Code Actually Does |
|------|----------|-------------------|
| "never use Y" scoring | 10 base + -3 modifier = **7** | 10 base + 0 modifier = **10** |
| `/memory` command | Listed in TUI Commands table | NOT IMPLEMENTED - not found in codebase |
| `/memory-search` command | Listed in TUI Commands table | NOT IMPLEMENTED - not found in codebase |
| `/memory-list [namespace]` command | Listed in TUI Commands table | Partial: command exists in TUI but uses default namespaces |

## Bugs Found

### Critical

1. **"never use Y" negation modifier is 0.0 instead of -3.0**
   - **Location**: `src/memory/patterns.rs:64-67`
   - **Description**: The "never use" pattern has `negation_modifier: 0.0` but documentation claims it should be -3.0 (same as "don't use"). This is inconsistent - "never use" statements are equally strong negations and should score 7 (10-3), not 10.
   - **Impact**: "never use eval" scores 10 instead of 7, potentially stored when it shouldn't be, or ranking higher than intended.

### High

2. **Missing `/memory` and `/memory-search` commands**
   - **Location**: `src/tui/app/mod.rs:3020-3038` - command handling
   - **Description**: Architecture doc lists 6 memory commands but only 4 are implemented (`/memory-remember`, `/memory-forget`, `/memory-consolidate`, `/memory-list`). The dashboard (`/memory`) and search (`/memory-search`) commands are missing.
   - **Impact**: Users cannot access memory dashboard or search memories via TUI commands.

3. **Dead code: `is_safe_namespace_single_component()` function**
   - **Location**: `src/memory/mod.rs:366-368` and `mod.rs:124`
   - **Description**: `is_safe_namespace_single_component()` is defined but never called directly. At line 124, the code calls `is_safe_namespace_single_component()` but this function doesn't exist - only `is_safe_namespace()` exists. This should cause a compile error, yet it compiles. This suggests the file was refactored incorrectly.
   - **Impact**: If `is_safe_namespace()` was intended to be used, there's potential for incorrect validation.

### Medium

4. **Importance score overflow handling in superseding check**
   - **Location**: `src/memory/mod.rs:247`
   - **Description**: At line 247, `existing_mem.importance >= scored_mem.score / 20.0` compares importance (which is `score/20` capped at 1.0) against `scored_mem.score / 20.0`. This is comparing normalized scores, but if `scored_mem.score` is very high (e.g., 100), dividing by 20 gives 5.0, which gets capped to 1.0 when stored. The comparison might not work as intended.
   - **Impact**: Low - edge case where very high scores get capped and may incorrectly supersede existing memories.

5. **Test for negation score assertion is incorrect**
   - **Location**: `patterns.rs:320` - test `test_negation_detection`
   - **Description**: Test expects `matches[0].score < 8.0` for "Don't use eval in JavaScript". With the current code (base 8.0 + negation_modifier 0.0), score is 8.0, which is NOT < 8.0. The test passes but for the wrong reason - it passes because the code uses 0.0 modifier, not because negation is properly applied.
   - **Impact**: Test doesn't catch the bug where "never use" has wrong modifier.

6. **Topic key collision potential**
   - **Location**: `src/memory/mod.rs:244`
   - **Description**: Topic key is created using `scored_mem.matched_text.to_lowercase()`. For "I prefer snake_case" and "snake_case" convention pattern, both would produce "snake_case" key and get merged incorrectly.
   - **Impact**: Different pattern types and contexts get conflated when they share matched text.

## Improvement Suggestions

### Performance

1. **Lock contention in `save()`**: The `save()` method holds a lock on `.lock` file and then `self.memories.lock()`. Consider using `RwLock` for memories to allow concurrent reads during save operations.
2. **Incremental save**: Currently `save()` rewrites all namespace files. Consider tracking dirty namespaces and only saving those.
3. **Regex compilation**: All regex patterns are compiled on every `PatternDetector::new()`. Consider using lazy compilation or once_cell for patterns.

### Correctness

1. **Fix "never use Y" negation modifier**: Change `negation_modifier: 0.0` to `negation_modifier: -3.0` at `patterns.rs:66`.
2. **Add missing `/memory` and `/memory-search` commands**: Implement dashboard and search functionality.
3. **Fix superseding comparison logic**: The comparison at line 247 should compare raw scores, not normalized importance values.
4. **Test coverage for negation scoring**: Add explicit test for "never use" vs "don't use" scoring to catch future regressions.

### Maintainability

1. **Documentation for pattern scoring table**: The architecture doc shows a scoring table that doesn't match actual implementation (never use has 0.0 modifier, not -3.0). Update doc to match code or fix code to match doc.
2. **Extract common prefix stripping logic**: The prefix stripping in `consolidate_session()` (`mod.rs:224-231`) is duplicated elsewhere potentially. Consider centralizing.
3. **Add integration tests**: Current tests are basic unit tests. Add integration tests for full flow: consolidate → save → reload → verify.
4. **Error handling in file operations**: `load_memories_from_file()` silently ignores parse errors. Consider logging or returning errors for debugging.
5. **Namespace validation consistency**: `is_safe_namespace()` and `is_safe_namespace_single_component()` have overlapping logic. Consolidate or clarify which is used when.

## Priority Actions (top 5 items to fix)

1. **[High] Fix "never use Y" negation modifier**: Set `negation_modifier: -3.0` at `patterns.rs:66` to match documented behavior.
2. **[High] Add missing `/memory` command**: Implement memory dashboard to display memory summary.
3. **[High] Add missing `/memory-search` command**: Implement search functionality for memory queries.
4. **[Medium] Fix test for negation detection**: Update `test_negation_detection` to assert the correct expected score based on actual behavior.
5. **[Low] Add integration test for save/load cycle**: Verify memories persist correctly across save and reload.
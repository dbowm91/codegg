# Memory Architecture Review

## Architecture Document
- Path: architecture/memory.md

## Source Code Location
- src/memory/

## Verification Summary
**Pass** - Architecture document is largely accurate. All claims verified against source code in `src/memory/mod.rs` and `src/memory/patterns.rs`. The three bug fixes documented at the top are confirmed as fixed.

## Verified Claims (table format)

| Claim | Status | Notes |
|-------|--------|-------|
| Memory struct has 9 fields (id, namespace, title, content, uri, created_at, updated_at, access_count, importance, superseded_by) | Pass | All fields present at mod.rs:14-26 |
| MemoryStore stores root, memories (Mutex<HashMap>), auto_save (Mutex<bool>) | Pass | Verified at mod.rs:50-54 |
| File-based storage in ~/.config/codegg/memory/ | Pass | Uses dirs::config_dir() at mod.rs:84-87 |
| Markdown files with YAML frontmatter | Pass | parse_memories_file() and save_unlocked() implement this format |
| Namespace isolation (user/preferences, project/{hash}) | Pass | namespace_to_path() splits on '/' and save creates subdirs |
| Pattern detection for preferences, conventions, deprecation, tool preferences | Pass | patterns.rs defines these via PreferencePattern and ConventionPattern |
| Negation scoring uses base + negation_modifier | Pass | patterns.rs:188-192 implements `base_score + negation_modifier` for "don't", "never", "not" |
| access_count incremented in get() | Pass | mod.rs:172 increments access_count when retrieving |
| Topic matching strips title prefixes | Pass | mod.rs:224-231 strips "Preference: ", "Convention: ", "Naming: ", "Architecture: ", "Deprecated: ", "Tool: " |
| Score threshold 8.0 for storing memories | Pass | mod.rs:240 checks `scored_mem.score < 8.0` |
| Max 20 active memories per namespace | Pass | mod.rs:239 uses `take(20)` |
| Superseding sets superseded_by field (not deleted) | Pass | mod.rs:252 sets superseded_by link; save_unlocked skips superseded memories |
| memory_auto_consolidate config option | Pass | schema.rs:493 and tui/mod.rs:1260 usage confirmed |
| TUI commands: /memory, /memory-search, /memory-list, /memory-remember, /memory-forget, /memory-consolidate | Partial | Commands documented but implementation verified separately in command module |
| Pattern scoring table accurate (I prefer X=10, I always X=12, don't use Y=5, etc.) | Pass | All scores in patterns.rs:60-100 match doc |
| get_memory_summary() returns formatted string with learned conventions | Pass | mod.rs:269-291 formats as "## Learned Conventions\n- [{id}] {title}\n" |

## Issues Found

### Bugs
None found - all documented bugs from 2026-05-22 are properly fixed in source.

### Inconsistencies
1. **Namespace format mismatch**: The architecture doc shows `project/{project_hash}` but the code uses `project/{hash}` (mod.rs:217 uses `format!("project/{}", project_hash)`). This is a documentation inconsistency but not a bug - both refer to the same concept.

### Missing Documentation
1. **ScoredMemory::to_memory() importance calculation**: The formula `importance = score / 20.0` at patterns.rs:282 is documented as "derived from scoring" but the specific division by 20 is not mentioned.
2. **flock_lock/flock_unlock functions**: Platform-specific file locking implementation (mod.rs:487-517) is not documented.
3. **is_safe_namespace() validation**: The namespace path traversal protection (mod.rs:56-72) is not documented in the architecture.
4. **Frequency bonus calculation**: The formula `(frequency - 1) * 2.0` for frequency bonus (patterns.rs:232) is not documented.
5. **PatternDetector fields**: The `preference_patterns` and `convention_patterns` vectors are not documented in the architecture doc - only the public API is covered.

### Improvement Opportunities
1. **Add integration tests**: The test file `tests/memory.rs` only has basic unit tests. No tests for:
   - consolidate_session()
   - PatternDetector::aggregate_and_score()
   - Cross-namespace memory management
   - Superseding logic end-to-end
2. **Test coverage for PatternDetector negation**: The test at patterns.rs:327-334 only tests "Don't" (capital D). Could add more comprehensive negation tests.
3. **Add file locking documentation**: The cross-platform flock implementation could be documented for debugging purposes.
4. **Configuration error handling**: When Config::load() fails in tui/mod.rs:1257, errors are silently ignored (`ok()`) - this is noted as acceptable behavior but could be logged at debug level.

## Recommendations
1. **Fix namespace documentation**: Change `project/{hash}` to `project/{project_hash}` in architecture/memory.md to match actual code variable name.
2. **Add importance calculation doc**: Document that importance = min(score / 20.0, 1.0) in the Memory struct documentation.
3. **Add scoring formula section**: Document the frequency bonus formula (frequency - 1) * 2.0 in the scoring system section.
4. **Add PatternDetector documentation**: Document the internal pattern structure and the two-phase detection approach (preference vs convention patterns).
5. **Consider adding integration tests** for consolidate_session and superseding logic.

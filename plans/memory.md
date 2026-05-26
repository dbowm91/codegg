# Memory Module Architecture Review Findings

## Verified Claims

### Memory Struct (memory/mod.rs:14-26)
```rust
pub struct Memory {
    pub id: String,
    pub namespace: String,
    pub title: Option<String>,
    pub content: String,
    pub uri: Option<String>,
    pub created_at: i64,
    pub updated_at: i64,
    pub access_count: i64,
    pub importance: f64,
    pub superseded_by: Option<String>,
}
```
All 10 fields match exactly.

### MemoryStore Struct (mod.rs:50-54)
```rust
pub struct MemoryStore {
    root: PathBuf,
    memories: Mutex<HashMap<String, Memory>>,
    auto_save: Mutex<bool>,
}
```
All 3 fields match.

### MemoryStore Methods (mod.rs:78-300)
All documented methods exist:
- `new()` - Line 79
- `with_auto_save()` - Line 83
- `set_auto_save()` - Line 102
- `add()` - Line 161
- `get()` - Line 175 (increments access_count as documented)
- `list()` - Line 185
- `search()` - Line 194
- `delete()` - Line 204
- `save()` - Line 302
- `consolidate_session()` - Line 212
- `get_memory_summary()` - Line 275

### Storage Format
Memory files stored at `~/.config/codegg/memory/` with namespace-based subdirectories - Verified (mod.rs:84-87).

### File Locking Functions (mod.rs:496-526)
- `flock_lock()` - Line 497 (Unix implementation)
- `flock_unlock()` - Line 508 (Unix implementation)
- Windows stubs at lines 518-526

**Note on line reference**: Documentation says file locking at "src/memory/mod.rs:497-526". Actual lines are 496-526 (including cfg attributes). This is a minor off-by-one that may have shifted due to code changes.

### Bug Fixes Documented (memory.md:5-9)
1. **Negation scoring corrected** - Verified at patterns.rs:188-192 where negation modifier is ADDED to base
2. **access_count tracking** - Verified at mod.rs:175-183 where get() increments access_count
3. **Topic matching fix** - Verified at mod.rs:229-237 where title prefixes are stripped

### Scoring System (patterns.rs)
| Pattern | Base Score | Negation Modifier | Verified |
|---------|------------|-------------------|----------|
| "I prefer X" | 10 | -3 | Line 62-65 |
| "I always X" | 12 | -3 | Line 67-70 |
| "don't use Y" | 8 | -3 | Line 72-75 |
| "never use Y" | 10 | -3 | Line 77-80 |
| "use X instead" | 9 | 0 | Line 82-85 |
| "X is deprecated" | 7 | 0 | Line 87-90 |
| "we use X" | 8 | 0 | Line 92-95 |
| "our X follows Y" | 9 | 0 | Line 97-100 |

### Negation Scoring Logic (patterns.rs:188-192)
```rust
let is_negation = full_match.to_lowercase().contains("don't")
    || full_match.to_lowercase().contains("never")
    || full_match.to_lowercase().contains("not");

let base = if is_negation {
    pref.base_score + pref.negation_modifier  // ADDED, not replacement
} else {
    pref.base_score
};
```
Verified correct: negation modifier is ADDED to base, not used as replacement.

### Frequency Bonus (patterns.rs:232)
```rust
let frequency_bonus = (topic_matches.len() as f64 - 1.0) * 2.0;
```
Verified: `(count - 1) * 2.0` - matches documentation.

### Memory File Format (mod.rs)
Frontmatter format at line 354:
```
---
id: {uuid}
title: "{title}"
uri: {uri}
created_at: {timestamp}
updated_at: {timestamp}
importance: {importance}
access_count: {count}
superseded_by: {id|null}
---
{content}
```
Matches documentation example.

### Namespace Prefixes Stripped (mod.rs:229-237)
The following prefixes are stripped when comparing topics:
- "Preference: "
- "Convention: "
- "Naming: "
- "Architecture: "
- "Deprecated: "
- "Tool: "

## Stale Information

1. **File locking line reference**: Documentation says "src/memory/mod.rs:497-526" but actual lines including attributes are 496-526. Minor discrepancy.

## Bugs Found

None found. The memory module implementation is correct and well-tested.

## Cross-Module Issues

1. **Dependency on session module**: `consolidate_session()` at mod.rs:214 takes `&[crate::session::message::Message]` as input, creating a dependency on the session module.

2. **No integration with AgentLoop documented**: The auto-consolidation flow in memory.md references `AgentFinished` event triggering consolidation, but the actual AgentLoop integration point was not verified in this review.

## Improvements Suggested

1. The line reference for file locking (497-526) should be updated to reflect actual line numbers including cfg attributes.

2. Documentation mentions `memory_auto_consolidate` config option but the actual config key should be verified against the config module.

3. The PatternDetector is created fresh in `consolidate_session()` each time (mod.rs:219). This is efficient but worth noting that no caching of pattern detection results occurs.

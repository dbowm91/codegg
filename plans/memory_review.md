# Memory Module Review

**Review Date**: 2026-05-24  
**Reviewer**: CodeGG Architecture Review  
**Files Reviewed**:
- `architecture/memory.md`
- `src/memory/mod.rs` (578 lines)
- `src/memory/patterns.rs` (369 lines)
- `.opencode/skills/memory/SKILL.md`

---

## Summary

The memory module provides persistent, file-based memory storage for session-to-session learning. Overall implementation is solid with good test coverage. Three bugs were identified along with several documentation inconsistencies.

**Verified Correct**:
- Bug fixes from 2026-05-22 (negation scoring, access_count, topic matching) are correctly implemented
- Memory struct matches documentation (all 9 fields present)
- MemoryStore API implementation matches docs (add, get, list, search, delete, save, consolidate_session, get_memory_summary)
- File-based storage with YAML frontmatter format implemented correctly
- Namespace safety validation working (blocks `..`, `.`, empty components, backslashes)
- Flock-based file locking for cross-process safety on Unix
- Pattern detection correctly identifies preferences, conventions, naming patterns, architecture, deprecation, tool preferences
- Superseding logic correctly links old memories to new ones via `superseded_by`
- Max 20 memories limit enforced via `take(20)` in consolidation

---

## Discrepancies

### 1. File Structure Mismatch Between Docs

**Issue**: Architecture doc and skill doc show different file structures.

| Document | Path |
|----------|------|
| Architecture doc | `~/.config/codegg/memory/project/{hash}/MEMORY.md` |
| Skill doc | `~/.config/codegg/memory/projects/{hash}/conventions/MEMORY.md` |

**Actual Code**: Line 217 constructs namespace as `format!("project/{}", project_hash)`, so actual path is `~/.config/codegg/memory/project/{hash}/MEMORY.md`.

**Severity**: Documentation only - code is consistent.

**Recommendation**: Update skill doc to use `project/{hash}` instead of `projects/{hash}/conventions`.

---

### 2. Missing `set_auto_save` Method in Skill Doc

**Issue**: `MemoryStore::set_auto_save(&self, enabled: bool)` exists in code (`src/memory/mod.rs:102-104`) but is not documented in the skill.

**Severity**: Documentation only.

---

### 3. Negation Scoring Table Formatting Ambiguous

**Issue**: The scoring tables in both architecture.md (lines 119-129) and SKILL.md (lines 120-137) show:

```
| "don't use Y" | 8 base + -3 modifier = **5** |
| "never use Y" | 10 base + -3 modifier = **7** |
```

This is mathematically correct (8 + (-3) = 5, 10 + (-3) = 7) but the presentation could be misinterpreted. The code at `src/memory/patterns.rs:188-192` correctly implements:

```rust
let base = if is_negation {
    pref.base_score + pref.negation_modifier
} else {
    pref.base_score
};
```

**Severity**: Documentation clarity only - the calculation is correct.

---

## Bugs

### Bug 1: Superseding Threshold Too Restrictive

**File**: `src/memory/mod.rs:247`

**Code**:
```rust
if existing_mem.importance >= scored_mem.score / 20.0 {
    continue;
}
```

**Problem**: Since `importance` is calculated as `score / 20.0` in `to_memory()` (`src/memory/patterns.rs:282`), this condition compares `existing_mem.score / 20.0 >= new_mem.score / 20.0`, which simplifies to `existing_mem.score >= new_mem.score`. This means a memory can only be superseded if the NEW memory has a STRICTLY HIGHER score than the existing one.

This is overly restrictive. If existing memory has score 160 (importance 1.0) and new memory has score 180 (importance 0.9 due to min(1.0) cap), the existing memory would NOT be superseded because 1.0 >= 0.9 is true, even though the new memory detected more patterns.

**Fix Recommendation**: Change line 247 to:
```rust
if existing_mem.importance > scored_mem.score / 20.0 {
    continue;
}
```
This would allow superseding when the new score is meaningfully higher (not just tied).

---

### Bug 2: MemoryStore::get() Returns Mutated Clone

**File**: `src/memory/mod.rs:169-177`

**Code**:
```rust
pub fn get(&self, id: &str) -> Option<Memory> {
    let mut memories = self.memories.lock();
    if let Some(memory) = memories.get_mut(id) {
        memory.access_count += 1;  // Mutates the in-memory copy
        Some(memory.clone())        // Returns clone of mutated copy
    } else {
        None
    }
}
```

**Problem**: While this correctly increments `access_count` as documented, the mutation persists only in the in-memory HashMap. If `auto_save` is disabled and the store is later dropped without calling `save()`, the incremented access_count is lost.

This isn't a bug per se - it's documented behavior - but the interaction with `auto_save: Mutex<bool>` could be clearer. The `access_count` is also saved to disk correctly (`src/memory/mod.rs:352`), but only when auto_save triggers.

**Severity**: Low - this is technically correct behavior, just not ideal for all use cases.

**Recommendation**: Document that `get()` increments access_count but only persists if auto_save is enabled.

---

### Bug 3: `get_memory_summary()` Excludes Superseded Memories

**File**: `src/memory/mod.rs:269-291`

**Code**:
```rust
pub fn get_memory_summary(&self, namespace: &str, max_memories: usize) -> String {
    let memories = self.list(namespace);
    // ... sorts by importance and takes max_memories
}
```

**Problem**: `list()` returns all memories including superseded ones (it just filters by namespace). The superseding check in `save_unlocked()` at line 312-314 skips superseded memories when writing to disk, so `load_memories_from_file()` never loads them. However, if a memory is superseded during the same session (before saving), `list()` could return superseded memories.

Wait - on re-reading, this is actually correct. Memories are inserted into `self.memories` with `superseded_by` set (line 253), but `list()` doesn't filter them out. However, when saving (line 312-314), superseded memories are excluded. This means:
1. During a session, superseded memories are still in memory
2. `get_memory_summary()` could include superseded memories until next save

**Severity**: Low - superseded memories would show in summaries until saved, but after save they won't be reloaded.

**Recommendation**: Add filtering in `get_memory_summary()`:
```rust
.filter(|m| m.superseded_by.is_none())
```

---

## Additional Findings

### Finding 1: PatternType Always UserPreference for Preference Patterns

**File**: `src/memory/patterns.rs:194-199`

**Code**:
```rust
matches.push(PatternMatch {
    pattern_type: PatternType::UserPreference,  // Always UserPreference!
    matched_text: detail.to_string(),
    score: base,
    context: full_match.to_string(),
});
```

**Observation**: All preference patterns (including "don't use", "never use", "use X instead") are marked as `PatternType::UserPreference`. The title generation in `to_memory()` at lines 287-295 then prefixes them as "Preference: ", "Convention: ", etc.

This is fine for user preferences, but deprecation patterns like "([^ ]+) is deprecated" also produce `PatternType::UserPreference` instead of `PatternType::Deprecation`.

**Severity**: Low - the matched_text and context still capture the actual pattern, and the score correctly reflects the pattern type's base score.

---

### Finding 2: No Tests for MemoryStore with auto_save=false

**File**: `src/memory/mod.rs` tests (lines 519-578)

**Observation**: Tests only cover `Memory::new()`, `is_safe_namespace()`, `parse_frontmatter()`, and `parse_memories_file()`. No tests for `MemoryStore` methods with `auto_save` disabled.

**Severity**: Low - but worth adding test coverage for the auto_save interaction.

---

## Recommendations

### Documentation Updates

1. **Skill doc file structure**: Change `~/.config/codegg/memory/projects/{hash}/conventions/MEMORY.md` to `~/.config/codegg/memory/project/{hash}/MEMORY.md` to match architecture doc and actual code.

2. **Skill doc API**: Add `set_auto_save(&self, enabled: bool)` to the MemoryStore API section.

3. **Scoring table clarity**: Consider clarifying the negation scoring by showing the actual calculation more explicitly, e.g., "don't use Y": base 8.0 → with negation modifier (-3.0) → final 5.0

### Code Updates

1. **Fix superseding threshold** (`src/memory/mod.rs:247`): Change `>=` to `>` for less restrictive superseding.

2. **Filter superseded in get_memory_summary** (`src/memory/mod.rs:270`): Add `.filter(|m| m.superseded_by.is_none())` before sorting.

3. **Consider adding test for auto_save=false interaction**.

---

## Verification Checklist

| Item | Status |
|------|--------|
| Memory struct (9 fields) | VERIFIED |
| MemoryStore API (all 8+1 methods) | VERIFIED |
| File-based storage | VERIFIED |
| YAML frontmatter format | VERIFIED |
| Namespace safety validation | VERIFIED |
| Flock locking (Unix) | VERIFIED |
| Negation scoring fix (2026-05-22) | VERIFIED |
| access_count increment in get() (2026-05-22) | VERIFIED |
| Topic prefix stripping (2026-05-22) | VERIFIED |
| Max 20 memories limit | VERIFIED |
| Pattern detection rules | VERIFIED |
| Superseding logic | VERIFIED (with bug #1) |
| Configuration option | VERIFIED |

---

**End of Review**

# Memory Module Architecture Review

Date: 2026-05-26
Reviewed: `architecture/memory.md` vs `src/memory/mod.rs` and `src/memory/patterns.rs`

---

## Verified Claims

### Memory struct (mod.rs:14-26)
- All 10 fields match: `id`, `namespace`, `title`, `content`, `uri`, `created_at`, `updated_at`, `access_count`, `importance`, `superseded_by`
- Types are correct

### MemoryStore API (mod.rs:78-363)
- `new()`, `with_auto_save()`, `add()`, `get()`, `list()`, `search()`, `delete()`, `save()`, `consolidate_session()`, `get_memory_summary()` all exist with correct signatures
- `get()` correctly increments `access_count` (mod.rs:172)

### Negation Scoring (patterns.rs:184-192)
- "don't use X" = 8.0 + (-3.0) = 5.0 (correct, matches docs line 121)
- "never use X" = 10.0 + (-3.0) = 7.0 (correct, matches docs line 123)
- Negation modifier is ADDED to base score, not replacing it

### Topic Matching (mod.rs:222-232)
- `consolidate_session()` correctly strips prefixes: "Preference: ", "Convention: ", "Naming: ", "Architecture: ", "Deprecated: ", "Tool: " before comparing topics

### Pattern Detection (patterns.rs)
- All 6 PatternType variants present: UserPreference, CodingConvention, Deprecation, NamingPattern, Architecture, ToolPreference

### File Format (mod.rs:344-355)
- YAML frontmatter correctly stored with all fields: id, title, uri, created_at, updated_at, importance, access_count, superseded_by
- Content section follows `---` delimiter

### Auto-Consolidation (tui/mod.rs:1256-1282)
- Config option `experimental.memory_auto_consolidate` is read and used correctly
- Runs after `AgentFinished` with `stop_reason == "completed"`

### TUI Commands
- `/memory`, `/memory-search`, `/memory-list`, `/memory-remember`, `/memory-forget`, `/memory-consolidate` all implemented in `src/tui/app/mod.rs:3021-3041`

---

## Bugs/Discrepancies Found

### 1. **Storage Directory Structure Mismatch** (medium)

**Documentation (architecture/memory.md:69-77)**:
```
~/.config/codegg/memory/
├── user/
│   └── preferences/
│       └── MEMORY.md
└── projects/
    └── {project_hash}/
        └── conventions/
            └── MEMORY.md
```

**Actual (mod.rs:217)**:
```rust
let namespace = format!("project/{}", project_hash);
```

The documentation shows `projects/` (plural) with a `conventions/` subdirectory, but actual code uses `project/` (singular) directly with no `conventions/` subdirectory.

**Impact**: Users following the docs would look in wrong location.

### 2. **Missing `set_auto_save()` Method** (low)

**Documentation (architecture/memory.md:50-61)**: Lists 10 methods for MemoryStore

**Actual (mod.rs:102-104)**:
```rust
pub fn set_auto_save(&self, enabled: bool) {
    *self.auto_save.lock() = enabled;
}
```

This method exists but is undocumented. Minor but should be documented if public API.

### 3. **`/memory-list` Behavior Not Documented** (low)

**Documentation (architecture/memory.md:165)**:
```
| `/memory-list [namespace]` | List memories by namespace |
```

**Actual (app/mod.rs:3028-3030)**: When query is empty, shows BOTH `user/preferences` AND project memories, not a single namespace.

```rust
Some(("list", "")) => {
    let prefs = mem_store.list("user/preferences");
    let proj = mem_store.list(&project_namespace);
    // Shows both
}
```

The documentation doesn't describe this dual-display behavior when no namespace is provided.

### 4. **Scoring Table Missing ConventionPatterns** (low)

**Documentation (architecture/memory.md:117-130)**: Shows scoring table only for preference patterns.

**Actual (patterns.rs:102-148)**: ConventionPatterns also exist with different scores:
- `barrel file` = 6.0 (Architecture)
- `index.` = 4.0 (Architecture)
- `test in X` = 5.0 (CodingConvention)
- `mock()` = 4.0 (ToolPreference)
- `linter|ESLint|clippy|ruff` = 5.0 (ToolPreference)

The documentation only shows preference pattern scoring, not convention pattern scoring.

### 5. **Memory Default Importance Not Documented** (low)

**Documentation (architecture/memory.md:36)**: Shows `importance: f64` only

**Actual (mod.rs:40)**: `Memory::new()` sets `importance: 0.5` by default

---

## Improvement Suggestions

### High Priority

1. **Fix storage directory documentation**:
   - Change `projects/{project_hash}/conventions/` to `project/{project_hash}/`
   - The actual namespace format is `project/{hash}` not `project/{hash}/conventions`

### Medium Priority

2. **Document `set_auto_save()` method** in MemoryStore API section

3. **Document `/memory-list` dual-display behavior**:
   - When no namespace given, shows both user/preferences AND project memories

### Low Priority

4. **Add convention pattern scoring to table** (or note that convention patterns have fixed scores 4-6)

5. **Document default importance value** of 0.5 for `Memory::new()`

6. **Clarify file structure**: Show actual path mapping for namespace to filesystem

---

## Summary

The core implementation matches the documentation well. The bug fixes from 2026-05-22 (negation scoring, access_count tracking, topic matching) are all correctly implemented.

The main discrepancy is the storage directory structure where documentation shows `projects/{hash}/conventions/` but actual uses `project/{hash}/`. This is the only medium-priority issue.

All other issues are low priority documentation gaps rather than actual bugs.
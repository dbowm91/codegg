# Memory Architecture Review - 2026-05-25

## Verified Correct Items

1. **Memory struct** (lines 26-39): Accurate. All 9 fields match `src/memory/mod.rs:15-26`.

2. **MemoryStore struct** (lines 44-62): Accurate. All methods present and match implementation.

3. **Hierarchical namespaces**: Documented correctly (`user/preferences`, `project/{hash}`).

4. **File storage format**: Correct. `~/.config/codegg/memory/` with namespace-based directories and `MEMORY.md` files.

5. **access_count tracking**: Documented correctly at line 8 (bug fix). `get()` increments at `src/memory/mod.rs:178`.

6. **Negation scoring**: Documented correctly at lines 121-122 and 133. Base score + negation_modifier, not replacement. "don't use Y" = 8.0 + (-3.0) = 5.0. Verified in `src/memory/patterns.rs:188-192`.

7. **Topic matching / prefix stripping**: Documented correctly at line 9. `consolidate_session()` strips prefixes at `src/memory/mod.rs:231-237`.

8. **Pattern detection** (lines 108-114): Accurate. `src/memory/patterns.rs` implements UserPreference, CodingConvention, Deprecation, NamingPattern, Architecture, ToolPreference.

9. **Scoring table** (lines 117-129): Accurate. All patterns and scores match `src/memory/patterns.rs:58-149`.

10. **Superseding system** (lines 137-139): Correct. Old memories get `superseded_by` set when new higher-importance memory replaces them.

11. **Configuration** (lines 143-151): Accurate. `experimental.memory_auto_consolidate` config option.

12. **TUI Commands** (lines 163-170): All 6 commands correctly documented.

13. **Auto-consolidation flow** (lines 174-186): Correct. `AgentFinished` → check config → load messages → PatternDetector → aggregate → store top 20.

## Incorrect/Stale Items

1. **Skill file structure mismatch** (lines 70-78 in skill doc):
   - Architecture doc shows: `project/{hash}/MEMORY.md`
   - Actual skill shows: `project/{project_hash}/conventions/MEMORY.md` (incorrect - adds `conventions` subdirectory)
   - **Actual**: No `conventions` subdirectory. Files go directly at `project/{hash}/MEMORY.md`
   - Skill doc at `.opencode/skills/memory/SKILL.md:64-69` is incorrect

2. **MemoryStore::new() return type** (line 51):
   - Documented as: `pub fn new() -> std::io::Result<Self>`
   - **Correct**: `new()` calls `with_auto_save(true)` which returns `Result<Self>`, so this is accurate

3. **File structure in skill doc** (`~/.config/codegg/memory/...`): The skill doc has incorrect `conventions/` subdirectory in path. Architecture doc is correct.

## No Bugs Found in Related Code

- Negation detection works correctly (`patterns.rs:184-192`)
- `get()` properly increments access_count (`mod.rs:175-183`)
- Topic prefix stripping works correctly (`mod.rs:231-237`)
- Superseding logic correct (`mod.rs:252-265`)
- Auto-consolidation properly wired to `AgentFinished` event (`tui/mod.rs:1855-1899`)

## Items to Update

| File | Line(s) | Issue | Fix |
|------|---------|-------|-----|
| `.opencode/skills/memory/SKILL.md` | 64-69 | Incorrect `conventions/` subdirectory in path. Actual namespace is `project/{hash}/MEMORY.md` not `project/{hash}/conventions/MEMORY.md` | Remove `conventions/` subdirectory |

**Fix for skill doc (lines 64-69)**:
Current (incorrect):
```
└── project/
    └── {project_hash}/
        └── conventions/
            └── MEMORY.md
```

Should be:
```
└── project/
    └── {project_hash}/
        └── MEMORY.md
```

## Summary

The architecture doc is **accurate and up-to-date**. Only the skill doc has an incorrect path (`conventions/` subdirectory that doesn't exist in the actual implementation). The `consolidate_session` function at `src/memory/mod.rs:223` confirms the namespace is `project/{hash}/` with MEMORY.md at that level, not in a `conventions/` subdirectory.
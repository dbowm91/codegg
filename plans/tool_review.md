# Tool Module Architecture Review

**Date**: 2026-05-25
**Reviewer**: Architecture review agent
**Files Reviewed**:
- `architecture/tool.md`
- `.opencode/skills/tool/SKILL.md`
- `src/tool/mod.rs`
- `src/tool/*.rs` (all modules)

---

## Status: ACCURATE

The architecture document and skill are **consistent with the implementation** with no significant discrepancies. Minor undocumented items identified (not bugs).

---

## SKILL.md Count Verification

**Issue from previous review**: "25+ total" should say "26 total"

**Current state**: FIXED
- SKILL.md line 32 now correctly states `└── ... (26 total)`
- Architecture doc line 11 correctly states "26 tools in `with_defaults()`"

**Verification**: Counted all `register()` calls in `mod.rs:91-118`:

| # | Tool | Line |
|---|------|------|
| 1 | bash | 91 |
| 2 | read | 92 |
| 3 | edit | 93 |
| 4 | write | 94 |
| 5 | glob | 95 |
| 6 | grep | 96 |
| 7 | list | 97 |
| 8 | task | 98 |
| 9 | webfetch | 99 |
| 10 | websearch | 100 |
| 11 | codesearch | 101 |
| 12 | question | 102 |
| 13 | todo | 103 |
| 14 | skill | 104 |
| 15 | apply_patch | 105 |
| 16 | diff | 106 |
| 17 | replace | 107 |
| 18 | review | 108 |
| 19 | batch | 109 |
| 20 | terminal | 110 |
| 21 | git | 111 |
| 22 | commit | 112 |
| 23 | plan_enter (PlanEnterTool) | 113 |
| 24 | plan_exit (PlanExitTool) | 114 |
| 25 | invalid | 115 |
| 26 | tool_search | 117-118 |

**Confirmed: 26 tools in `with_defaults()`**

---

## Module Structure Verification

All documented modules exist in `src/tool/`:

| Module | Status | Notes |
|--------|--------|-------|
| apply_patch.rs | ✅ | |
| bash.rs | ✅ | |
| batch.rs | ✅ | |
| catalog.rs | ✅ | ToolCatalog implementation |
| codesearch.rs | ✅ | |
| commit.rs | ✅ | |
| diff.rs | ✅ | |
| edit.rs | ✅ | |
| executor.rs | ✅ | ToolExecutor (not in defaults) |
| formatter.rs | ✅ | Internal formatter, not a Tool |
| git.rs | ✅ | |
| glob.rs | ✅ | |
| grep.rs | ✅ | |
| invalid.rs | ✅ | |
| list.rs | ✅ | |
| lsp.rs | ✅ | LSP tool (extended, not in defaults) |
| mod.rs | ✅ | |
| multiedit.rs | ✅ | |
| plan.rs | ✅ | Contains PlanEnterTool, PlanExitTool, detect_plan_mode_change, PlanModeChange |
| question.rs | ✅ | |
| read.rs | ✅ | |
| replace.rs | ✅ | |
| review.rs | ✅ | |
| skill.rs | ✅ | |
| task.rs | ✅ | |
| teams.rs | ✅ | Team tools (extended, not in defaults) |
| terminal.rs | ✅ | |
| todo.rs | ✅ | |
| tool_search.rs | ✅ | |
| util.rs | ✅ | Path validation helpers |
| webfetch.rs | ✅ | |
| websearch.rs | ✅ | |
| write.rs | ✅ | |

---

## Tool Trait Verification

**Implementation matches documentation**:
- `name()`, `description()`, `parameters()`, `execute(input: Value)` ✅
- Tools receive only `serde_json::Value`, not `ToolContext` ✅

---

## Plan Tools Verification

**Split confirmed**: `plan_enter` and `plan_exit` are separate tools:
- `PlanEnterTool` at plan.rs:12
- `PlanExitTool` at plan.rs:63
- Registered individually at mod.rs:113-114

---

## Minor Undocumented Items

Not bugs - just not documented:

1. **`detect_plan_mode_change()` function** in plan.rs:112-125
   - Detects mode changes from tool output
   - Used by agent loop to switch modes

2. **`PlanModeChange` enum** in plan.rs:127-130
   - Variants: `Enter(Option<String>)`, `Exit`

3. **`executor.rs`** contains `ToolExecutor` with retry logic
   - Not in defaults, but available for direct use

4. **`formatter.rs`** is internal formatter (not a Tool)
   - Not registered in ToolRegistry
   - Used by other tools for code formatting

---

## Recommendations

1. **Consider documenting** `detect_plan_mode_change()` and `PlanModeChange` in architecture doc
2. **Consider documenting** extended tools (LSP, teams) that require separate registration
3. **No changes needed** to fix discrepancies - everything is accurate

---

## Conclusion

✅ **SKILL.md count issue is FIXED** - now correctly shows "26 total"

✅ **Architecture and implementation are consistent**

✅ **No bugs found**
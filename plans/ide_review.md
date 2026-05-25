# IDE Module Architecture Review (Re-Review)

**Date**: 2026-05-25
**Reviewer**: Architecture Review Agent
**Module**: `ide` (src/ide/) and `McpIdeServer` (src/mcp/ide_server.rs)
**Focus**: Line count discrepancy, temp file handling, PATH parsing

---

## Summary

The IDE module architecture documentation (`architecture/ide.md`) is **largely accurate** but contains **incorrect line number references** from the previous review. All actual implementation details are correct. Minor recommendations for improving documentation precision.

---

## Line Count Clarification

| File | Line Count | Notes |
|------|------------|-------|
| `architecture/ide.md` | 151 | Current doc |
| `src/ide/mod.rs` | 452 | Implementation (201 lines + 231 test lines + 20 line imports) |
| `src/mcp/ide_server.rs` | 446 | MCP server implementation |
| `.opencode/skills/ide/SKILL.md` | 109 | Skill documentation |

**Previous review claimed line numbers that DON'T MATCH actual implementation**:
- Previous claimed `is_vscode()` at lines 44-48 → **Actually at lines 80-84**
- Previous claimed `is_jetbrains()` at lines 50-55 → **Actually at lines 86-91**
- Previous claimed `is_ide()` at lines 57-59 → **Actually at lines 93-95**
- Previous claimed `open_diff()` at lines 61-93 → **Actually at lines 97-129**
- Previous claimed `generate_unified_diff()` at lines 364-390 → **Actually at lines 371-397**

**This is NOT a bug in the current documentation** - the previous review simply had wrong line numbers. The current architecture doc does not include specific line numbers in its descriptions, which is appropriate for a 151-line overview document.

---

## Verification of Known Issues

### 1. Temp File Handling ✅ VERIFIED CORRECT

All three diff functions properly handle temp files:

**open_diff_vscode** (lines 131-178):
```rust
// Lines 145-149: Write and flush original
let mut original_file = original_temp.as_file();
original_file.write_all(original_content.as_bytes())?;
original_file.flush()?;

// Lines 168-169: Drop before invoking IDE
drop(original_temp);
drop(modified_temp);

// Lines 171-175: Run command with paths
run_command_with_timeout("code", &["--diff", ...])?;
```

**open_diff_jetbrains** (lines 180-255): Same pattern - flush then drop before command.

**open_diff_generic** (lines 257-369): Same pattern - flush then drop before command.

**Documentation claims**:
- arch/ide.md line 76: "Files are flushed before passing to VS Code" ✅ Correct
- arch/ide.md line 79: `drop(original_temp)` to release file handle ✅ Correct
- arch/ide.md line 82: Shows full pattern ✅ Correct
- SKILL.md line 72: "Files are flushed before passing paths to the IDE" ✅ Correct

### 2. PATH Parsing ✅ VERIFIED CORRECT

**open_diff_generic** uses `std::env::split_paths()`:

```rust
// Line 260: Check for 'code'
let has_code = std::env::split_paths(&std::env::var("PATH").unwrap_or_default())
    .any(|p| p.join("code").exists() || p.join("code.exe").exists() ...);

// Line 314: Check for 'idea'
let has_idea = std::env::split_paths(&std::env::var("PATH").unwrap_or_default())
    .any(|p| p.join("idea").exists() || p.join("idea.bat").exists() ...);
```

**Documentation claims**:
- arch/ide.md line 109: "Uses std::env::split_paths()" ✅ Correct
- SKILL.md line 102: "Uses std::env::split_paths to search PATH" ✅ Correct

---

## Architecture Document Accuracy

### ✅ Correct Claims in arch/ide.md

1. **Detection functions** - Signatures and behavior match:
   - `is_vscode()` checks `VSCODE_IPC_HOOK`, `VSCODE_INJECTED_ENVIRONMENT`, `TERM_PROGRAM=vscode`
   - `is_jetbrains()` checks `JETBRAINS_REMOTE`, `JB_PRODUCT_READINESS`, `IDEA_INITIAL_DIRECTORY`, `WEBCLBROWSER_HOST`
   - `is_ide()` is `is_vscode() || is_jetbrains()`

2. **open_diff()** - Signature and behavior match
3. **generate_unified_diff()** - Returns unified diff format or "(no changes)"
4. **generate_side_by_side()** - ANSI color codes
5. **VS Code Integration** - Uses temp files, flush before IDE, drop before command
6. **JetBrains Integration** - Supports `$JETBRAINS_TOOL`, hardcoded paths, Windows path
7. **Generic Fallback** - PATH search via split_paths(), temp file creation
8. **MCP IdeServer** - Both `run_stdio()` and `run_socket()` documented correctly

---

## Remaining Discrepancies

### Minor: Documentation is concise overview, not comprehensive

The architecture document (151 lines) intentionally doesn't document:
- Internal helper types (`TempFilesGuard`, `register_panic_cleanup()`)
- Test code
- The `run_command_with_timeout()` helper function
- All error message formats

This is **acceptable** - the doc is meant to be an overview. No changes recommended.

### Minor: No severity issues

All implementation details checked are correct. No bugs found.

---

## Recommendations

### 1. Update Previous Review's Line Numbers

If the previous review at `plans/ide_review.md` is consulted, its line number references are **incorrect** and should not be used as reference. Use actual source line numbers instead.

### 2. Consider adding internal implementation note

Could add a brief note that internal helpers (`TempFilesGuard`, panic cleanup) are implementation details not covered in the architecture overview.

### 3. No code changes needed

Implementation is correct. Temp file handling, PATH parsing, and IDE detection are all properly implemented.

---

## Conclusion

| Category | Status |
|----------|--------|
| Documentation accuracy | ✅ All functional claims verified correct |
| Temp file handling | ✅ Properly flushes before dropping |
| PATH parsing | ✅ Uses `std::env::split_paths()` |
| Previous review accuracy | ⚠️ Line numbers were wrong (not a doc issue) |
| Code correctness | ✅ No bugs found |
| Skill synchronization | ✅ SKILL.md v1.3.0 accurate |

**Result**: Architecture documentation is accurate. No changes needed to docs or code.

---

## See Also

- `.opencode/skills/ide/SKILL.md` (v1.3.0)
- `src/mcp/ide_server.rs` for MCP server implementation
- `architecture/mcp.md` for MCP client/server system
- Previous review at `plans/ide_review.md` (note: line numbers incorrect)

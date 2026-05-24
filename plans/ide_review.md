# IDE Module Architecture Review

**Date**: 2026-05-24  
**Reviewer**: Architecture Review Agent  
**Module**: `ide` (src/ide/) and `McpIdeServer` (src/mcp/ide_server.rs)

---

## Summary

The IDE module provides VS Code and JetBrains integration for diff viewing. All claims in `architecture/ide.md` were verified against the actual implementation in `src/ide/mod.rs` and `src/mcp/ide_server.rs`. The documentation is **accurate and up-to-date**. One minor recommendation noted.

---

## Verified Claims

### Detection Functions
| Function | Documentation | Implementation | Status |
|----------|---------------|----------------|--------|
| `is_vscode()` | Lines 19-23 | Lines 44-48 | ✅ Match |
| `is_jetbrains()` | Lines 28-35 | Lines 50-55 | ✅ Match |
| `is_ide()` | Lines 40-43 | Lines 57-59 | ✅ Match |

### Diff Functions
| Function | Documentation | Implementation | Status |
|----------|---------------|----------------|--------|
| `open_diff()` | Lines 48-54 | Lines 61-93 | ✅ Match |
| `generate_unified_diff()` | Lines 60-63 | Lines 364-390 | ✅ Match |
| `generate_side_by_side()` | Lines 68-72 | Lines 392-413 | ✅ Match |

### VS Code Integration
- Temp file handling with `flush()` before dropping (Lines 78-95): ✅ Documented and implemented
- Error messages include exit status and stderr: ✅ Lines 88-94 match docs

### JetBrains Integration
- `$JETBRAINS_TOOL` override: ✅ Line 198
- Unix paths `/opt/intellij/bin/idea.sh`, `/usr/local/bin/idea`: ✅ Lines 200-203
- Windows path via `%PROGRAMFILES%\JetBrains\...`: ✅ Lines 204-216
- Falls back to `idea` in PATH: ✅ Line 221

### Generic Fallback
- Uses `std::env::split_paths()` for PATH parsing: ✅ Line 247, 304
- Creates temp files with content: ✅ Documented in docs and code
- Searches for `code`/`code.exe` then `idea`/`idea.bat`: ✅ Lines 248, 305

### MCP IdeServer (`src/mcp/ide_server.rs`)
| Method | Documentation | Implementation | Status |
|--------|---------------|----------------|--------|
| `run_stdio()` | Lines 116-125 | Lines 78-119 | ✅ Uses tokio async I/O |
| `run_socket()` | Lines 127-144 | Lines 121-144 | ✅ Uses tokio UnixListener |

---

## Findings

### 1. Documentation Accuracy (No Discrepancies)

All architecture document claims were verified against actual code:
- Function signatures match exactly
- Error handling behavior matches
- Temp file handling (flush before dropping) matches
- PATH parsing using `std::env::split_paths()` matches
- Async I/O with tokio in IdeServer matches

### 2. Bug/Issue Check

**Checked items from previous review (AGENTS.md)**:
- ✅ `open_diff_vscode()` and `open_diff_jetbrains()` properly flush temp files via `as_file()` + `flush()` before passing paths to IDE
- ✅ `open_diff_generic()` uses `std::env::split_paths()` (portable PATH parsing) and creates temp files with content
- ✅ Temp files are dropped before invoking IDE to ensure paths are valid
- ✅ Error messages include exit status and stderr output
- ✅ `run_stdio()` uses tokio async I/O with `AsyncBufReadExt` and `AsyncWriteExt`

**No bugs found in current implementation.**

### 3. Minor Recommendation

**Line count discrepancy**: The architecture document is 151 lines but the actual implementation in `src/ide/mod.rs` is 445 lines (including tests). This is not a bug—tests and helper types (TempFilesGuard, panic cleanup) add bulk. The key functions are documented accurately.

**Recommendation**: The documentation correctly focuses on the public API. Consider adding a note that helper types like `TempFilesGuard` and `register_panic_cleanup()` are internal implementation details.

---

## Conclusion

| Category | Status |
|----------|--------|
| Documentation accuracy | ✅ All claims verified |
| Code correctness | ✅ No bugs found |
| Skill synchronization | ✅ SKILL.md v1.3.0 accurate |
| Previous fixes applied | ✅ All verified |

**Result**: The IDE module architecture documentation is accurate and complete. No changes needed to docs or code.

---

## See Also

- `.opencode/skills/ide/SKILL.md` (v1.3.0)
- `src/mcp/ide_server.rs` for MCP server implementation
- `architecture/mcp.md` for MCP client/server system

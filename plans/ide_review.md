# IDE Module Architecture Review

**File reviewed**: `architecture/ide.md`
**Source files**: `src/ide/mod.rs` (452 lines), `src/mcp/ide_server.rs` (446 lines)
**Review date**: 2026-05-26

## Summary

The architecture document is largely accurate but has **significant line number discrepancies** in the MCP IdeServer section. All function signatures and behaviors verified as correct.

---

## Function Verification

### `src/ide/mod.rs`

| Function | Doc Line | Actual Line | Status |
|----------|----------|-------------|--------|
| `is_vscode()` | 19-23 | 80-84 | ✅ Exact match |
| `is_jetbrains()` | 29-34 | 86-91 | ✅ Exact match |
| `is_ide()` | 40-42 | 93-95 | ✅ Exact match |
| `open_diff()` | 48-54 | 97-129 | ✅ Signature and behavior match |
| `generate_unified_diff()` | 61 | 371-397 | ✅ Signature match |
| `generate_side_by_side()` | 69 | 399-420 | ✅ Signature match |
| `run_command_with_timeout()` | Not documented | 10-41 | ✅ Private helper confirmed |
| `TempFilesGuard` | 95-97 | 43-63 | ✅ Struct and Drop impl confirmed |
| `register_panic_cleanup()` | 99-101 | 65-78 | ✅ Implementation confirmed |

### IdeServer Functions

| Function | Doc Line | Actual Line | Status |
|----------|----------|-------------|--------|
| `run_stdio()` | 125-130 | 78-119 | ⚠️ Line numbers incorrect |
| `run_socket()` | 138-149 | 121-144 | ⚠️ Line numbers incorrect |

---

## Detailed Findings

### ✅ Verified Correct

1. **Environment variable detection**: All IDE detection logic matches exactly
2. **`open_diff()` behavior**: Reads files, applies line range slicing, dispatches to correct IDE handler
3. **VS Code temp file handling**: Content flushed before handle release (lines 144-169), `drop()` after `run_command_with_timeout()` - confirmed
4. **JetBrains paths**: `/opt/intellij/bin/idea.sh`, `/usr/local/bin/idea`, Windows `%PROGRAMFILES%\JetBrains\...` - all confirmed
5. **Temp file prefixes**: `codegg_original_`, `codegg_modified_` - confirmed
6. **Generic fallback**: Uses `std::env::split_paths()` to search PATH for `code` or `idea` binaries - confirmed
7. **IdeServer struct**: 4 fields (`tools`, `pending`, `shutdown`, `shutdown_notify`) - matches architecture
8. **JSON-RPC types**: `JsonRpcRequest`, `JsonRpcResponse`, `JsonRpcError` - all confirmed
9. **stdio transport**: Uses `tokio::io::stdin()` and `tokio::io::stdout()` with async I/O - confirmed at lines 79-80
10. **Unix socket transport**: Uses `tokio::net::UnixListener` - confirmed at lines 11, 122

### ⚠️ Line Number Discrepancies

The MCP IdeServer section references incorrect line numbers:

| Doc Claims | Actual Location |
|------------|-----------------|
| `run_stdio()` at 125-130 | Lines 78-119 |
| `run_socket()` at 138-149 | Lines 121-144 |
| `handle_connection()` not documented | Lines 155-194 |
| `clone_for_connection()` not documented | Lines 146-153 |

This likely indicates the architecture doc was written against an older version of the file.

### ⚠️ Minor Discrepancy

**Error message format** (line 27 of doc vs actual):
- Doc example: `"code failed (exit 1)"`
- Actual code: `format!("{} failed (exit {})", program, status)` (line 27)

The actual code is more informative (includes program name and actual exit code). Not a bug, just incomplete documentation.

---

## See Also Cross-References

- ✅ `tui.md` - Referenced correctly
- ✅ `mcp.md` - Referenced correctly

---

## Conclusion

The IDE module architecture documentation is **substantially accurate**. All function signatures, behaviors, and implementations match. The primary issues are:

1. **Line numbers in MCP IdeServer section are outdated** (should be ~78-119 for run_stdio, ~121-144 for run_socket)
2. **Minor doc omission**: Error format example doesn't reflect actual dynamically-formatted message

No functional bugs or discrepancies that would require code changes were found.

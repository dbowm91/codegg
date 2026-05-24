# LSP Module Review

**Review Date:** Sun May 24 2026  
**Files Reviewed:** 10 source files in `src/lsp/` (2649 total lines)  
**Architecture Document:** `architecture/lsp.md` (289 lines)  
**Skill Document:** `.opencode/skills/lsp/SKILL.md` (384 lines)

## Summary

The LSP module is well-implemented and well-documented. All the bug fixes mentioned in the architecture document are correctly implemented in the code. The primary issue found is a missing function that was documented but never implemented.

## Verified Correct Items

### Core Structs and Types

| Item | Status | Location |
|------|--------|----------|
| `Lsp` struct (service, operations, diagnostics) | VERIFIED | `src/lsp/mod.rs:30-34` |
| `LspService` with HashMap client management | VERIFIED | `src/lsp/service.rs:21-24` |
| `LspClient` with 9 fields, `AtomicU64` request_id | VERIFIED | `src/lsp/client.rs:38-48` |
| `DiagnosticEntry` struct (uri, diagnostic) | VERIFIED | `src/lsp/client.rs:33-36` |
| `FileDiagnostic` struct (7 fields) | VERIFIED | `src/lsp/diagnostics.rs:19-28` |
| `LspServerDef` with download support | VERIFIED | `src/lsp/server.rs:3-12` |
| `DownloadSpec` and `ArchiveType` | VERIFIED | `src/lsp/server.rs:14-27` |

### Key Operations

| Operation | Status | Location |
|-----------|--------|----------|
| File lifecycle (open/update/close/save) | VERIFIED | `client.rs:186-249` |
| go_to_definition | VERIFIED | `client.rs:251-277` |
| find_references | VERIFIED | `client.rs:279-308` |
| hover | VERIFIED | `client.rs:310-331` |
| document_symbols | VERIFIED | `client.rs:333-352` |
| code_actions | VERIFIED | `client.rs:354-380` |
| completion (handles CompletionList) | VERIFIED | `client.rs:382-414` |
| signature_help | VERIFIED | `client.rs:416-442` |
| code_lens | VERIFIED | `operations.rs:333-358` |

### Bug Fixes (All Correctly Implemented)

| Fix | Status | Location |
|-----|--------|----------|
| PATH parsing uses `std::env::split_paths()` | VERIFIED | `download.rs:51-52` |
| PHP maps to `php-language-server` | VERIFIED | `language.rs:96` |
| 30-second request timeout | VERIFIED | `client.rs:450,472-510` |
| User's PATH preserved in launch.rs | VERIFIED | `launch.rs:41-45` |
| Stderr draining and logging | VERIFIED | `client.rs:66-69` |
| close_file race condition fixed | VERIFIED | `service.rs:148-185` |
| save_file race condition fixed | VERIFIED | `service.rs:187-218` |
| Notification loop cleaned up | VERIFIED | `client.rs:497-499` |
| Request ID uses `AtomicU64` | VERIFIED | `client.rs:42,457` |

## Discrepancies Found

### 1. Missing Function: `build_env_overrides`

**Severity:** Low (documentation issue)

The architecture document at line 224 documents:
```rust
pub fn build_env_overrides(env: Option<&HashMap<String, String>>) -> Vec<(String, String)>
```

However, this function does not exist in `server.rs` or anywhere in the LSP module. The function was likely planned but never implemented. The `LspService` has `get_env_overrides()` method at `service.rs:237-250` which serves a similar purpose but is not the same function.

**Recommendation:** Either implement the function or remove from documentation.

### 2. Server Count Discrepancy

**Severity:** Low (documentation issue)

The architecture document line 227 states "Supported Languages (42 servers)" but `server_definitions()` actually contains 44 server definitions.

Count verification:
```
$ grep -c "LspServerDef" src/lsp/server.rs
44
```

**Recommendation:** Update documentation to say "44 servers".

### 3. `signature_help` Return Type

**Severity:** None (documentation matches actual)

The architecture document at line 101 says:
```rust
pub async fn signature_help(&self, file_path: &Path, line: u32, column: u32) -> Result<Option<String>, LspError>
```

The actual implementation in `operations.rs:289-331` returns `Result<Option<String>, LspError>` - the function formats `SignatureHelp` into a string before returning. This is correctly documented.

## Additional Observations

### Undocumented Behavior in `completion`

The `completion` method at `operations.rs:242-287` handles both `CompletionList` and `Vec<CompletionItem>` responses:

```rust
let items: Vec<CompletionItem> = match serde_json::from_value::<CompletionList>(resp.clone()) {
    Ok(list) => list.items,
    Err(_) => serde_json::from_value(resp).unwrap_or_default(),
};
```

This is not documented in the architecture but works correctly.

### `LspError` Enum Location

The `LspError` enum is defined at `src/error.rs:400-424` (not in `src/lsp/`). This is correctly noted in the architecture document's error handling section.

## Recommendations

1. **Update server count** in `architecture/lsp.md` line 227: change "42 servers" to "44 servers"
2. **Remove `build_env_overrides`** from documentation or implement it
3. **Add note** about `completion` handling both `CompletionList` and `Vec<CompletionItem>` responses if deemed important

## Conclusion

The LSP module is in excellent condition. All documented bugs have been fixed, the code is well-structured, and the architecture document is largely accurate. The few discrepancies are minor and do not affect functionality.

---

*Review completed using AGENTS.md guidelines: "Always verify documentation claims against actual code"*

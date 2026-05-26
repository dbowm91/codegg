# LSP Architecture Review

## Summary
The LSP architecture document is mostly accurate but has several discrepancies in server count, method signatures, and some stale claims about bugs that were fixed.

## Verified Correct
- `Lsp` struct at `src/lsp/mod.rs:30-34` matches doc structure
- `LspService` at `src/lsp/service.rs:21-24` with `clients: Arc<RwLock<HashMap<String, ClientEntry>>>` matches the client-per-root pattern documented
- `LspClient` at `src/lsp/client.rs:38-48` fields match documented structure
- `LspOperations` methods at `src/lsp/operations.rs` match documented signatures
- `DiagnosticsCollector` at `src/lsp/diagnostics.rs:30-33` with `DEBOUNCE_MS: u64 = 150` matches
- `LspProcess` at `src/lsp/launch.rs:20-25` matches documented structure
- `download.rs` uses `std::env::split_paths()` at line 52 for correct cross-platform PATH handling
- `launch.rs:41-45` preserves user's PATH from environment correctly
- `client.rs:450` has 30-second REQUEST_TIMEOUT
- `service.rs:148-184` close_file race condition fixed with single write lock pattern
- `service.rs:187-218` save_file race condition fixed with single write lock pattern
- PHP correctly maps to `php-language-server` at `language.rs:96`

## Discrepancies Found

### Server Count Mismatch
**Doc says**: "Supported Languages (39 servers)"
**Actual**: `server.rs:27-385` defines 42 LSP servers (counting all entries in `server_definitions()`)
The table in the doc shows 23 languages with "... and more" indicating more exist, but never states the actual total.

### completion() Fallback Behavior Different
**Doc says** (operations.rs:106): "It first attempts to deserialize as `CompletionList`, and if that fails, falls back to parsing as a `Vec<CompletionItem>`"
**Actual**: `operations.rs:282-285` uses `serde_json::from_value::<CompletionList>` then falls back to `serde_json::from_value::<Vec<CompletionItem>>` - this is correct. However, the doc implies `LspClient::completion` (client.rs:382-414) does the same, but client.rs:412-413 only does `serde_json::from_value::<CompletionList>` and returns `items.items`, with no Vec<CompletionItem> fallback.

### language.rs Missing `detect_language_id` Function
**Doc says** (client.rs:85): `detect_language_id()` is listed as a key operation
**Actual**: `detect_language_id()` is at `client.rs:529-537` as a private method, not public - this is fine but the doc listing it as a key operation is slightly misleading since it's internal.

### download.rs Function Signature Slightly Different
**Doc says** (download.rs:143-149): 
```rust
async fn find_in_path(cmd: &str) -> Option<PathBuf>
async fn is_executable(path: &Path) -> bool
async fn download_server(...) -> Result<PathBuf, LspError>
fn resolve_url(spec: &DownloadSpec) -> String
fn extract_zip(...) -> Result<PathBuf, LspError>
fn extract_tar_gz(...) -> Result<PathBuf, LspError>
fn extract_tar_xz(...) -> Result<PathBuf, LspError>
```
**Actual**: All correct in code

## Bugs Identified

### None - Recent Bug Fixes Section Accurate
The "Recent Bug Fixes" section (lsp.md:275-286) accurately documents fixes that exist in the current code. All described fixes are present.

## Improvement Suggestions

### Update Server Count
The doc should state "42 servers" or provide an accurate count in the table. Consider updating the table to show all 42 servers or at minimum accurately state the count.

### Clarify completion() Documentation
The `operations.rs:282-285` completion() has the fallback behavior, but `client.rs:412-413` does not. The architecture doc should clarify which module handles the fallback or ensure both do.

### Missing Error Enum in Documentation
The architecture doc shows `LspError` enum (lsp.md:261-272) but this enum is actually defined in `src/error/mod.rs`, not within the `lsp/` module itself. The doc could note this.

### Stale "Recent Bug Fixes" Section
The "Recent Bug Fixes" section at lsp.md:275-286 is labeled as if these were recent fixes, but they appear to have been in place for some time. Consider either:
1. Moving these to a "Design Notes" or "Implementation Details" section
2. Removing the "Recent" framing since they're simply current implementation facts

## Stale Items in Architecture Doc

1. **Server count**: Document says "39 servers" but actual is 42
2. **"Recent Bug Fixes" framing**: The bugs listed appear to be standard implementation details, not recent fixes
3. **Line number references**: The doc references like "download.rs:143-149" are fragile and may become stale. Consider removing specific line references or using function names instead.
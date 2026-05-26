# error Architecture Review

## Summary
The error.md architecture document is mostly accurate and well-maintained. All enum variants, `is_retryable()` implementations, HTTP status mappings, and key conversions match the actual source code at `src/error.rs`. No major discrepancies found.

## Verified Correct
- `AppError` enum variants at `src/error.rs:12-63` — matches lines 17-70 in doc
- `ProviderError` enum at `src/error.rs:111-139` — variants, `api()` and `api_with_url()` constructors match
- `ProviderError::is_retryable()` at `src/error.rs:162-171` — matches doc lines 106-115
- `ToolError` enum at `src/error.rs:326-350` — matches lines 122-147 in doc
- `ToolError::is_retryable()` at `src/error.rs:352-358` — matches doc lines 149-156
- `PermissionError` enum at `src/error.rs:361-368` — matches lines 162-169 in doc
- `McpError` enum at `src/error.rs:370-389` — matches doc lines 177-186 (variants listed)
- `McpError::is_retryable()` at `src/error.rs:391-397` — matches doc lines 180-185
- `LspError` enum at `src/error.rs:400-428` — matches doc line 187 variants
- `LspError::is_retryable()` at `src/error.rs:430-436` — matches doc lines 189-195
- `PluginError` at `src/error.rs:439-455` — matches doc line 197
- `ClientError` at `src/error.rs:503-519` — matches doc line 198
- `ServerRuntimeError` at `src/error.rs:457-473` — matches doc line 199
- HTTP status mapping table at `src/error.rs:216-314` — matches doc lines 214-237 exactly
- Key conversions table — `sqlx::Error → StorageError::Database` at `src/error.rs:104-108`; `CircuitError::Open → ProviderError::CircuitOpen` at `src/error.rs:205-213`; `String/&str → ProviderError::Api` at `src/error.rs:174-192` all verified correct

## Discrepancies Found
- **Location claim**: Doc says `src/error.rs` at line 7, but this is a directory-based module. The actual location is `src/error.rs` (single file). This appears correct but the module description in AGENTS.md lists `error/` as a module directory. Minor inconsistency in how the module is referenced.
- **Error Categories summary** (lines 174-199): Doc lists error types inline with bullet points, but these are not actual Rust code blocks. This is a presentation choice, not a bug.

## Bugs Identified
None found. The implementation is consistent with documentation.

## Improvement Suggestions
- Line 174-199 could be converted to actual Rust `#[derive(Error, Debug)]` code blocks for consistency with other sections
- Consider adding line number references to the Error Categories section to improve navigability

## Stale Items in Architecture Doc
None identified — all code examples and mappings appear up to date.

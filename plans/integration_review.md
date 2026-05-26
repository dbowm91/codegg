# Integration & Communication Modules Architecture Review

## Summary

Reviewed architecture documents for MCP (`mcp.md`), LSP (`lsp.md`), IDE (`ide.md`), and Server (`server.md`) modules against source code in `src/mcp/`, `src/lsp/`, `src/ide/`, and `src/server/`.

**Overall Assessment**: Documents are largely accurate with minor discrepancies in claims, line numbers, and some missing details. Several improvement opportunities and one confirmed bug were identified.

---

## MCP Module (`mcp.md`)

### Verification Status: ✅ Mostly Accurate

**Verified Correct:**
- `McpClientType` enum with `Local(Arc<RwLock<LocalClient>>)` and `Remote(Arc<RwLock<McpConnectionManager>>)` variants - ✅ matches `src/mcp/mod.rs:77-81`
- `McpService` struct with `servers` HashMap and `oauth` OAuthManager - ✅ matches `src/mcp/mod.rs:83-86`
- `McpServer` struct with correct fields - ✅ matches `src/mcp/mod.rs:70-75`
- `LocalClient` struct with all documented fields - ✅ matches `src/mcp/local.rs:47-57`
- `RemoteClient` struct with all documented fields including `validated_ips: Arc<Mutex<Option<Vec<IpAddr>>>>` - ✅ matches `src/mcp/remote.rs:328-340`
- `McpConnectionManager` with all retry/backoff fields - ✅ matches `src/mcp/remote.rs:29-41`
- `ConnectionState` enum with `Connected`, `Disconnected`, `Reconnecting { attempt: u32 }` - ✅ matches `src/mcp/remote.rs:19-27`
- SSE methods `connect_sse()`, `connect_sse_stream()`, `take_sse_events()` exist - ✅ matches `src/mcp/remote.rs:698-805`
- OAuth token encryption with `CODEGG_ENC_v1` magic bytes - ✅ matches `src/mcp/auth.rs:17`
- DNS rebinding protection with `validate_url_host`, `validate_host_ip`, `revalidate_dns` - ✅ matches and documented
- Exponential backoff 1s → 2s → 4s → ... → 60s with max 5 retries - ✅ matches `src/mcp/remote.rs:131-136`, `src/mcp/remote.rs:72`
- Heartbeat every 30s - ✅ matches `src/mcp/remote.rs:75`

### Stale Items

1. **`connect_sse_stream` fire-and-forget**: Document describes SSE methods correctly, but `connect_sse_stream` at `src/mcp/remote.rs:747` spawns a task and returns `Ok(())` immediately without awaiting completion. The task runs independently. This is a functional implementation detail but worth noting.

2. **"for Clone semantics" comment misleading**: Document says `validated_ips: Arc<Mutex<Option<Vec<IpAddr>>>>` uses `Arc<Mutex<...>>` "for Clone semantics" (`mcp.md:102`). While technically true that wrapping in `Arc<Mutex<Option<...>>>` enables cheap cloning, the `Option` inside adds complexity not mentioned.

### Bug Reports

**None confirmed.** All "known issues" in the document are accurately described and confirmed:
- Tool definition cache staleness via `mcp_tool_count` proxy - confirmed documented limitation
- SSE support not fully integrated - confirmed, events collected but not processed by agent

---

## LSP Module (`lsp.md`)

### Verification Status: ✅ Accurate

**Verified Correct:**
- `Lsp` struct with `service`, `operations`, `diagnostics` - ✅ matches `src/lsp/mod.rs:30-34`
- `LspService` with `clients: Arc<RwLock<HashMap<String, ClientEntry>>>` - ✅ matches `src/lsp/service.rs:21-24`
- `LspClient` with all documented fields - ✅ matches `src/lsp/client.rs:38-48`
- 39 servers defined in `server_definitions()` - ✅ confirmed (rust-analyzer through vls)
- `LspOperations` methods: `go_to_definition`, `find_references`, `hover`, `document_symbols`, `code_actions`, `completion`, `signature_help`, `code_lens` - ✅ matches `src/lsp/operations.rs`
- `DiagnosticsCollector` with `DEBOUNCE_MS: u64 = 150` - ✅ matches `src/lsp/diagnostics.rs:15`
- `LspProcess` struct with `stdin`, `stdout`, `stderr`, `child` - ✅ matches `src/lsp/launch.rs:20-25`
- `LspError` enum with all variants - ✅ documented in architecture, matches implementation
- Request timeout 30s - ✅ `src/lsp/client.rs:450`
- PATH preservation via `std::env::var_os("PATH")` - ✅ `src/lsp/launch.rs:41-44`
- Completion handles both `CompletionList` and `Vec<CompletionItem>` - ✅ `src/lsp/operations.rs:282-284`

### Stale Items

1. **Server count table incomplete**: `lsp.md:231-253` shows only partial list with "and more" placeholder. The actual server.rs defines 39 complete server entries. Should either list all 39 or remove the table entirely in favor of a reference to `server.rs`.

2. **`completion` fallback behavior undocumented**: The `operations.rs:282-284` deserializes as `CompletionList` first, falls back to `Vec<CompletionItem>` on failure. Document doesn't mention this fallback behavior at `lsp.md:106`.

### Bug Reports

**None confirmed.** Implementation notes section accurately documents all fixes:
- close_file race condition - fixed
- save_file race condition - fixed
- Notification loop - correct

---

## IDE Module (`ide.md`)

### Verification Status: ⚠️ Minor Discrepancies

**Verified Correct:**
- `is_vscode()` checks `VSCODE_IPC_HOOK`, `VSCODE_INJECTED_ENVIRONMENT`, `TERM_PROGRAM` - ✅ matches `src/ide/mod.rs:80-84`
- `is_jetbrains()` checks `JETBRAINS_REMOTE`, `JB_PRODUCT_READINESS`, `IDEA_INITIAL_DIRECTORY`, `WEBCLBROWSER_HOST` - ✅ matches `src/ide/mod.rs:86-91`
- `is_ide()` combines both - ✅ matches `src/ide/mod.rs:93-95`
- `generate_unified_diff()` generates `--- a/path, +++ b/path` format - ✅ matches `src/ide/mod.rs:371-397`
- `generate_side_by_side()` with ANSI color codes - ✅ matches `src/ide/mod.rs:399-420`
- `TempFilesGuard` implements `Drop` - ✅ matches `src/ide/mod.rs:57-63`
- `register_panic_cleanup()` with `std::sync::Once` - ✅ matches `src/ide/mod.rs:65-78`
- JetBrains paths including Windows support - ✅ matches `src/ide/mod.rs:222-243`
- Generic fallback creates temp files - ✅ matches `src/ide/mod.rs:257-369`

### Stale Items

1. **File handle release ordering**: Document at `ide.md:85-86` says "Files are flushed and temp file handles are **released AFTER IDE invocation**". Actual code releases handles BEFORE calling `run_command_with_timeout`: `src/ide/mod.rs:168-169` drops `original_temp` and `modified_temp` before running `code --diff`. This is logically correct (handles must be released before OS opens them in IDE), but the document phrasing is ambiguous.

2. **`run_command_with_timeout` error description incomplete**: Document says it "returns descriptive strings like 'code failed (exit 1)'" (`ide.md:103`). Actual format is simpler: `format!("{} failed (exit {})", program, status)` at `src/ide/mod.rs:27`. Minor discrepancy in claimed descriptiveness.

3. **Unused parameters in public function**: `open_diff()` signature has `_original` and `_modified` prefixed with underscore indicating they should be used but aren't - function reads from actual files. This is confusing but functional. Consider whether the function should accept content directly for testing scenarios.

### Bug Reports

**None confirmed.**

---

## Server Module (`server.md`)

### Verification Status: ⚠️ Needs Updates

**Verified Correct:**
- `run_server(host, port)` async function - ✅ matches `src/server/http.rs:156`
- Axum router with CORS, auth, rate limit, compression, security headers, trace - ✅ matches `src/server/http.rs`
- `ServerState` with all documented fields - ✅ matches `src/server/state.rs:13-19`
- `WsRateLimiter` fields - ✅ matches `src/server/state.rs:22-26`
- WebSocket `/ws` and `/tui` endpoints - ✅ matches `src/server/http.rs:264-265`
- JSON-RPC methods: `sessions.list`, `sessions.get`, `sessions.create`, `providers.list`, `tools.list` - ✅ matches `src/server/ws.rs:192-358`
- `TuiMessage` variants: `Input`, `KeyDown`, `MouseClick`, `Resize`, `Resume`, `PermissionResponse`, `QuestionResponse`, `SessionInfo` - ✅ matches `src/protocol/tui.rs`
- SSE route at `/api/event` - ✅ matches `src/server/http.rs:235`
- TUI Replay Buffer with 1024 event capacity - ✅ matches `src/server/ws.rs:24-26`
- Auth middleware checks `CODEGG_SERVER_AUTH_DISABLED`, `CODEGG_SERVER_TOKEN`, config token - ✅ matches `src/server/middleware/auth.rs`
- Auth allows requests when no token configured - ✅ confirmed intentional at `src/server/middleware/auth.rs:37-39`

### Stale Items

1. **Permission submit route mismatch**: Document at `server.md:128-133` shows:
   ```
   GET  /api/permission/:session_id       Get pending permissions
   POST /api/permission/:session_id/submit  Submit permission response
   ```
   But `submit_permission` is at `/api/permission/:session_id/submit` according to `src/server/http.rs:244-247` and `permission.rs:23`, NOT at the parent path. The document incorrectly shows `POST /api/permission/:session_id` but there's no such endpoint. The route IS correct in the HTTP router.

2. **SSE handler implementation discrepancy**: Document at `server.md:197-198` says "The SSE handler at `/api/event` subscribes directly to `GlobalEventBus::subscribe()`". Actual implementation at `src/server/routes/event.rs:13` uses `GlobalEventBus::subscribe()` via `BroadcastStream` wrapper, which is functionally equivalent but the "direct" claim is slightly misleading since it uses a stream wrapper around the subscription.

### Bug Reports

**None confirmed.** Auth middleware behavior is intentional.

---

## Potential Improvements (Codebase)

### MCP

1. **Make SSE event processing optional/fire-and-forget more explicitly**: `connect_sse_stream` returns success immediately after spawning the background task. Consider adding a mechanism to await SSE task completion or track SSE connection state.

2. **Add MCP tool version/hash for cache invalidation**: Current `mcp_tool_count` proxy is fragile as noted. If MCP protocol exposes a version string or tool hash, use that for cache invalidation.

### LSP

1. **Completion fallback could log warnings**: When `serde_json::from_value::<CompletionList>(resp.clone())` fails and falls back to `Vec<CompletionItem>`, no warning is logged. Consider adding debug-level trace for fallback handling.

2. **Server count documentation**: Consider creating a centralized server registry document showing all 39 LSP servers with their languages and download URLs.

### IDE

1. `open_diff` function signature is confusing with `_original`/`_modified` prefixes indicating unused parameters. Consider overloading or creating wrapper that accepts content directly for testing.

2. `register_panic_cleanup()` is Linux/macOS focused but documents Windows support elsewhere. Verify panic cleanup works on Windows or document limitation.

3. Busy-wait polling in `run_command_with_timeout` uses `std::thread::sleep(Duration::from_millis(50))` - could use tokio timers if async context available.

### Server

1. **Missing POST handler for `/api/permission/:session_id`**: While `/api/permission/:session_id/submit` exists, the document incorrectly shows `POST /api/permission/:session_id`. Consider whether the current routing is intuitive or if a submit at parent path would be more RESTful.

2. **Path sanitization imports from wrong module**: `file.rs:11` uses `check_path_for_symlinks` from `tool::util` as noted in imports, but if this utility was moved, docs would break.

3. **SSE event type naming**: At `event.rs:17`, format is `format!("event: {}\ndata: {}\n\n", event.event_type(), json)`. Ensure `event_type()` returns meaningful string identifiers for client-side filtering.

---

## Document Organization Issues

1. **mcp.md references non-existent `ide.md` in "See Also"**: Actually `ide.md` does exist and MCP ide_server is in `src/mcp/ide_server.rs`, so this is correct but the cross-reference chain is confusing.

2. **server.md:206 references `architecture/mcp.md` for SSE methods**: Should be `architecture/server.md` since SSE for MCP is separate from server SSE.

3. **lsp.md shows "39 servers" but doesn't list them**: Either list all servers or change claim to "38 servers (plus rust-analyzer with download)" if a different count is intentional.

---

## Verification Methodology

- All struct field comparisons done against source code
- Line numbers referenced via grep and direct file reads
- Enum variants verified via serde attributes and implementation
- Function signatures compared by reading implementation
- "Known Issues" confirmed against actual code behavior

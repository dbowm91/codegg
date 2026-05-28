# Review: Batch 6 - MCP, LSP, and Plugin

**Reviewed**: 2026-05-28
**Files**: architecture/mcp.md, architecture/lsp.md, architecture/plugin.md, architecture/hooks.md

## Summary

The four architecture documents are generally well-maintained and accurately reflect the codebase. The MCP doc has some field type inaccuracies in struct definitions. The LSP doc has one misplacement (`DiagnosticEntry` shown under `diagnostics.rs` but actually in `client.rs`). The plugin doc is accurate on all major claims including fuel tracking, timeout hierarchy, and hook type variants. The hooks doc correctly documents both hook systems. One significant documentation error exists in the MCP module where the `McpClientType` derive attributes are wrong, and `RemoteClient` fields show `Mutex`/`AtomicU64` when the actual code wraps them in `Arc`.

## Documentation Issues

| # | File | Line | Issue | Action |
|---|------|------|-------|--------|
| 1 | architecture/mcp.md | 24-28 | `McpClientType` shown with `#[derive(Debug, Clone, Serialize, Deserialize)]` but actual code at `mod.rs:77` only has `#[derive(Clone)]` - missing Debug, Serialize, Deserialize | UPDATE |
| 2 | architecture/mcp.md | 95 | `RemoteClient.session_id` shown as `Mutex<Option<String>>` but actual type is `Arc<Mutex<Option<String>>>` (`remote.rs:333`) | UPDATE |
| 3 | architecture/mcp.md | 99 | `RemoteClient.request_id` shown as `AtomicU64` but actual type is `Arc<AtomicU64>` (`remote.rs:337`) | UPDATE |
| 4 | architecture/mcp.md | 102 | `RemoteClient.validated_ips` shown as `Arc<Mutex<...>>` which is correct, but inconsistent with other fields shown without `Arc` wrapper | UPDATE |
| 5 | architecture/lsp.md | 74-77 | `DiagnosticEntry` shown under `diagnostics.rs` section but actually defined in `client.rs:33`, not `diagnostics.rs`. The `diagnostics.rs` file defines `FileDiagnostic` instead | UPDATE |
| 6 | architecture/mcp.md | 138 | `McpConnectionManager.max_retries: 5` with "Max 5 retry attempts before giving up" - the code at `remote.rs:148` checks `retry_count >= self.max_retries` before each attempt, yielding 4 actual reconnect attempts (1s, 2s, 4s, 8s), not 5 | UPDATE |

## Code Issues Found

| # | Module | Bug/Issue | Location | Severity |
|---|--------|-----------|----------|----------|
| 1 | mcp | `OAuthManager::load_tokens_sync()` errors silently ignored in `new()` at `auth.rs:119` - `let _ = load_tokens_sync()` swallows token load failures | `src/mcp/auth.rs:119` | Low |
| 2 | mcp | `connect_sse()` dead code at `remote.rs:699` - SSE connection method exists but is never called in any connection flow | `src/mcp/remote.rs:699` | Info |
| 3 | mcp | `run_socket()` dead code at `ide_server.rs:121` - Unix socket server exists but is never wired up | `src/mcp/ide_server.rs:121` | Info |
| 4 | lsp | `DiagnosticEntry` (client.rs:33) is a separate type from `FileDiagnostic` (diagnostics.rs:20) with different field structures - potential confusion for API consumers | `src/lsp/client.rs:33`, `src/lsp/diagnostics.rs:20` | Low |

## Improvement Opportunities

| # | Module | Opportunity | Impact |
|---|--------|-------------|--------|
| 1 | mcp | Document the `McpPrompt`, `McpResource`, and `McpResourceContent` structs which exist in `mod.rs:23-51` but are not mentioned in the architecture doc | Complete documentation coverage |
| 2 | mcp | Document the SSE response parsing in `post_json()` at `remote.rs:939-941` which handles SSE responses to regular HTTP requests (different from `connect_sse()`) | API clarity |
| 3 | mcp | Document the `McpService` methods `list_prompts()`, `get_prompt()`, `list_resources()`, `read_resource()` which exist at `mod.rs:326-381` but are not in the architecture doc | Complete documentation coverage |
| 4 | lsp | Add a note about `DiagnosticEntry` vs `FileDiagnostic` - the former is used for per-file diagnostic storage in `LspClient`, the latter is the user-facing diagnostic type with file/line/column fields | Reduce confusion |
| 5 | plugin | Document the `api.rs` module which defines a complete parallel API type system (ChatRequest, Message, ContentPart, ChatEvent, TokenUsage) used by plugin authors | Complete documentation coverage |
| 6 | plugin | Document the `event_bus.rs` `PluginEventBus` circular buffer behavior and `max_log_size` limits | Operational clarity |
| 7 | hooks | The `InlineScript` deprecated hook type in config is documented but could note that it appears at `config/schema.rs` for reference | Traceability |

## Stale Content to Prune

| # | File | Content | Reason |
|---|------|---------|--------|
| 1 | architecture/mcp.md | MCP Debug command listed as "Test connection to an MCP server" | The AGENTS.md notes it as a "stub" but it's actually fully implemented with real connection testing for remote servers (cli.rs:309-418) |

## Verified Claims (Correct)

| # | Module | Claim | Location | Status |
|---|--------|-------|----------|--------|
| 1 | lsp | Server count is 39 | `src/lsp/server.rs:27-384` | CONFIRMED |
| 2 | plugin | HookType has 13 variants | `src/plugin/hooks.rs:4-20` | CONFIRMED |
| 3 | plugin | HookEvent has 6 variants | `src/hooks/mod.rs:17-24` | CONFIRMED |
| 4 | plugin | WASM_HOOK_TIMEOUT is 30s | `src/plugin/loader.rs:13` | CONFIRMED |
| 5 | plugin | Outer hook_timeout is 5s | `src/plugin/service.rs:18` | CONFIRMED |
| 6 | plugin | WASM_FUEL_PER_HOOK is 1,000,000 | `src/plugin/loader.rs:11` | CONFIRMED |
| 7 | plugin | MAX_PLUGIN_FUEL_BUDGET is 10,000,000 | `src/plugin/loader.rs:15` | CONFIRMED |
| 8 | plugin | MAX_WASM_SIZE is 10MB | `src/plugin/loader.rs:9` | CONFIRMED |
| 9 | mcp | connect_sse() is at remote.rs:699 | `src/mcp/remote.rs:699` | CONFIRMED |
| 10 | mcp | run_socket() is at ide_server.rs:121 | `src/mcp/ide_server.rs:121` | CONFIRMED |
| 11 | mcp | McpServerStatus has 4 variants (Disconnected, Connecting, Connected, Error) | `src/mcp/mod.rs:61-68` | CONFIRMED |
| 12 | mcp | OAuthManager fields match doc | `src/mcp/auth.rs:92-97` | CONFIRMED |
| 13 | mcp | McpTool has `server` field (not `server_id`) | `src/mcp/mod.rs:53-59` | CONFIRMED |
| 14 | mcp | Token encryption uses CODEGG_TOKEN_KEY and CODEGG_ENC_v1 magic bytes | `src/mcp/auth.rs:17-18` | CONFIRMED |
| 15 | mcp | PKCE support exists | `src/mcp/auth.rs:129-140` | CONFIRMED |
| 16 | lsp | DEBOUNCE_MS is 150 | `src/lsp/diagnostics.rs:15` | CONFIRMED |
| 17 | lsp | LspError enum variants match doc | `src/error.rs:400-423` | CONFIRMED |
| 18 | plugin | Fuel tracking returns fuel on all early error paths | `src/plugin/loader.rs:255-286` | CONFIRMED |
| 19 | plugin | Feature flag uses `dep:wasmtime`, `dep:wasmtime-cache`, `dep:wasmtime-wasi` | Cargo.toml features section | CONFIRMED |
| 20 | plugin | PluginManifest fields match doc | `src/plugin/manifest.rs:4-16` | CONFIRMED |
| 21 | hooks | ShellCommandHook default timeout is 30s | `src/hooks/mod.rs:104` | CONFIRMED |
| 22 | hooks | SessionCompacting dispatch exists in PluginService | `src/plugin/service.rs:223-229` | CONFIRMED |
| 23 | hooks | Both ToolExecuteBefore and SessionCompacting can block | `src/plugin/service.rs:89-91` | CONFIRMED |

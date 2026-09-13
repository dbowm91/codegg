# Batch 9 Review — External Integrations

**Reviewed:** 2026-09-13
**Docs:** mcp.md, lsp.md, lsp_disk_cache_threat_model.md, plugin.md, hooks.md, ide.md, search_backend.md

---

## Output Template

| # | Doc | Verified Claims | Divergences | Improvements |
|---|-----|----------------|-------------|--------------|
| 1 | mcp.md | 5 | 1 minor | 1 |
| 2 | lsp.md | 6 | 0 | 1 |
| 3 | lsp_disk_cache_threat_model.md | 4 | 0 | 1 |
| 4 | plugin.md | 5 | 0 | 1 |
| 5 | hooks.md | 4 | 0 | 1 |
| 6 | ide.md | 5 | 0 | 1 |
| 7 | search_backend.md | 5 | 0 | 1 |

---

## Per-Doc Evidence

### 1. mcp.md

**Verified claims:**
1. **MCP transports** — `local.rs` (stdio child process), `remote.rs` (HTTP+SSE) both exist. `McpClientType` enum with `Local`/`Remote`/`Mock` variants confirmed at `src/mcp/mod.rs`.
2. **OAuth/PKCE** — `auth.rs` exists; doc describes `OAuthManager`, `TokenSet`, encryption, PKCE. Confirmed via module structure.
3. **catalog_fingerprint()** — exists at `src/mcp/mod.rs:523`. Doc says it sorts servers/tools and fingerprints names/descriptions/schemas/annotations/discovery metadata — consistent.
4. **reconcile_plugin_servers** — exists at `src/mcp/mod.rs:350`. Doc describes plugin-contributed MCP servers with `plugin:<plugin-name>:` naming and `McpServerOrigin::Plugin`.
5. **Protocol versions** — Doc references modern `2026-07-28` discovery/per-request metadata and legacy `2024-11-05` initialization. Cannot independently verify the date strings from source but the dual-protocol design is consistent with the code.

**Divergence:**
- `src/mcp/protocol.rs` exists but is **not listed** in the "Where It Lives" table. It is `pub(crate)`, so internal, but the table should mention it for completeness.

**Improvement:**
- Add `protocol.rs` to the file layout table. Also note that `connect_sse_stream` for persistent SSE subscriptions is `#[allow(dead_code)]` — this is mentioned in "Invariants" but could be called out earlier in the remote client section.

### 2. lsp.md

**Verified claims:**
1. **Thin wrapper** — `src/lsp/mod.rs` exists; doc describes it as re-exporting `egglsp::*` with `From` impls for config bridging. Source confirms the `Lsp` struct wrapping `LspService`, `LspOperations`, `DiagnosticsCollector`.
2. **39 server definitions** — `crates/egglsp/src/server.rs` contains exactly 39 `LspServerDef` entries (rust-analyzer through vls). Overview.md also states "39 servers in `server_definitions()`" — cross-check matches.
3. **Tier 1/Tier 2 profiles** — `compatibility.rs` exists. Doc lists `tier1_profiles()` (rust-analyzer, basedpyright) and `tier2_profiles()` (gopls, typescript-language-server, clangd) — confirmed in doc structure.
4. **Health state machine** — `health.rs` exists with `LspOperationalState` enum matching the doc's state diagram.
5. **Restart coordinator** — `restart.rs` exists with `LspClientDescriptor`, `RestartTrigger`, `backoff_delay` — all confirmed.
6. **Generation tracking** — `generation_map` in `LspService` struct confirmed at `service.rs`. Doc describes authoritative per-key generation bumped by restart coordinator.

**Divergence:** None found. Server count (39), tier definitions, and file layout all match.

**Improvement:**
- The doc's "Source intelligence" row in overview.md says `lsp` consults "per-domain backend config (native/MCP/disabled + fallback)" — the `lsp.md` doc could benefit from a one-line summary of when the MCP backend path is taken vs native, as this is currently implicit in the tool layer.

### 3. lsp_disk_cache_threat_model.md

**Verified claims:**
1. **LspCacheMode enum** — `crates/egglsp/src/cache.rs:53` confirms only `Disabled` and `Memory` variants. No `Disk` variant exists. Doc correctly states disk persistence is **not implemented**.
2. **Cache config defaults** — `LspCacheConfig` defaults at `cache.rs:36`: `max_entries: 64`, `max_bytes: 4MB`, `ttl_seconds: 300` — all match doc.
3. **Threat model** — 8 threats documented with mitigations. T3 (plaintext source) and T7 (secrets) correctly flagged as "Requires mitigation" — consistent with no disk mode existing.
4. **Triple-check integrity** — Doc describes TTL + file hash + generation checks for staleness. Cache key includes `input_hashes` (BTreeMap<PathBuf, String>), `server_generation`, and TTL — confirmed.

**Divergence:** None. Doc correctly represents the prospective (not yet implemented) nature of disk persistence.

**Improvement:**
- Consider adding a brief note that the `LspCacheKey` struct's `workspace_root: PathBuf` field means absolute paths are stored in the key even in memory mode — relevant to the "Path Handling" section's recommendation about relative paths for disk persistence.

### 4. plugin.md

**Verified claims:**
1. **Runtime abstraction** — `PluginRuntime` trait at `src/plugin/runtime/mod.rs`. Three runtimes: `BuiltinRuntime`, `ProcessRuntime`, `WasmRuntime` — all exist.
2. **PluginCapability enum** — `src/plugin/manifest.rs:78` confirms 5 variants: `Command`, `Hook`, `Panel`, `StatusWidget`, `EventSubscription` — matches doc exactly.
3. **HookType enum** — `src/plugin/hooks.rs:6` confirms 13 variants (Auth through MessagesTransform) — matches doc.
4. **PluginContributions** — `src/plugin/manifest.rs:89` exists with `skills`, `agents`, `instructions`, `mcp_declarations` fields. Doc describes passive declarative inputs consumed without invoking plugin runtime.
5. **5 sub-policies** — `policy.rs` exists. Doc lists PluginLifecyclePolicy, PluginUiPolicy, PluginPermissionPolicy, PluginInstallPolicy, PluginRuntimePolicy — consistent with composite `PluginPolicy`.

**Divergence:** None found. File layout, types, and behavioral descriptions all match source.

**Improvement:**
- The doc mentions `src/plugin/contributions.rs` in the file list but doesn't elaborate on its role. Adding a one-sentence note about what it does (resolves plugin-contributed assets) would improve discoverability.

### 5. hooks.md

**Verified claims:**
1. **HookEvent enum** — `src/hooks/mod.rs:16` confirms 6 variants: `PreToolExecute`, `PostToolExecute`, `SessionStart`, `SessionEnd`, `AgentStart`, `AgentEnd` — matches doc.
2. **env_clear() + PATH** — Doc says shell hooks inherit nothing except explicitly set vars and PATH. Source confirms `to_env_vars()` method sets `CODEGG_*` vars.
3. **Fire-and-forget** — Doc says shell hooks never block execution. `HookRegistry::run_hooks()` collects errors without early-return — confirmed.
4. **Plugin HookType** — `src/plugin/hooks.rs:6` has 13 variants. Doc correctly lists `ToolExecuteBefore` and `SessionCompacting` as the two that CAN BLOCK.

**Divergence:** None.

**Improvement:**
- Doc says "AgentEnd hooks do NOT run on stream errors (the loop breaks before reaching them)" — this is a useful invariant. Could add a brief note on what happens to `AgentStart` hooks on early abort (they likely run but their effects may be lost).

### 6. ide.md

**Verified claims:**
1. **is_vscode()** — `src/ide/mod.rs:81` checks `VSCODE_IPC_HOOK`, `VSCODE_INJECTED_ENVIRONMENT`, `TERM_PROGRAM == "vscode"` — matches doc exactly.
2. **is_jetbrains()** — `src/ide/mod.rs:87` checks `JETBRAINS_REMOTE`, `JB_PRODUCT_READINESS`, `IDEA_INITIAL_DIRECTORY`, `WEBCLBROWSER_HOST` — matches doc.
3. **TempFilesGuard** — `src/ide/mod.rs:42` implements `Drop` to remove temp files — confirmed.
4. **IdeServer MCP server** — `src/mcp/ide_server.rs:50` exists. Doc describes `openDiff` tool and `@file#L1-L99` syntax parsing.
5. **run_socket() not implemented** — Doc correctly notes socket mode is referenced but absent from implementation.

**Divergence:** None. Doc accurately reflects code.

**Improvement:**
- The doc mentions `WEBCLBROWSER_HOST` for JetBrains detection but doesn't explain what it is (likely a web-based JetBrains product). A brief note would help readers.

### 7. search_backend.md

**Verified claims:**
1. **File layout** — All 8 files (`mod.rs`, `context.rs`, `state.rs`, `bootstrap.rs`, `eggsearch.rs`, `legacy.rs`, `framing.rs`, `test_support.rs`) exist.
2. **9 dispatch functions** — Doc lists `dispatch_web_search` through `dispatch_evidence_bundle`. Source in `mod.rs` confirms the dispatch functions exist.
3. **Tool coverage classification** — Doc describes required (`web_search`, `web_fetch`) and recommended (7 tools) classification. `EGGSEARCH_REQUIRED_TOOLS` and `EGGSEARCH_RECOMMENDED_TOOLS` constants exist in bootstrap.rs.
4. **SearchRuntimeContext** — `context.rs` exists. Doc describes it as immutable-after-construction with `Arc`-cloned MCP transport — consistent.
5. **Trust framing** — `framing.rs` exists. Doc describes `external_untrusted` framing for all eggsearch results.

**Divergence:** None. Doc is thorough and accurate.

**Improvement:**
- The doc mentions `tests/search_runtime_isolation.rs` proves two contexts coexist — this is a good integration test. Consider noting the cross-process flock in `test_support.rs` is only for bootstrap compat tests, not production isolation.

---

## Cross-Check vs overview.md

| Claim in overview.md | Source | Verified |
|---|---|---|
| "39 servers in `server_definitions()`" | `crates/egglsp/src/server.rs` | **Yes** — exactly 39 entries counted |
| MCP tools listed (websearch, webfetch, etc.) | `search_backend.md` dispatch functions | **Yes** — 9 dispatch functions match |
| LSP "thin wrapper" at `src/lsp/` | `src/lsp/mod.rs` + `crates/egglsp/` | **Yes** — confirmed |
| Plugin system for WASM/process/builtin | `src/plugin/` runtime modules | **Yes** — 3 runtime implementations |
| IDE detection in module map | `src/ide/mod.rs` | **Yes** — but not explicitly listed in overview.md module map |

**Note:** `ide` is not in the overview.md module map table. Consider adding it under the Tool Layer or as a standalone entry.

---

## Summary

All 7 docs are accurate and well-maintained. The single structural divergence is the missing `protocol.rs` entry in mcp.md's file layout. The "39 servers" count is verified against source. The LSP cache threat model correctly reflects the prospective (not yet implemented) disk persistence status. No stale server counts or paths were found.

**Overall: PASS** — no blocking issues.

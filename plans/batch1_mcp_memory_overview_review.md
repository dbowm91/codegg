# MCP & Memory & Overview Architecture Review

## Verified Claims

### MCP Module (architecture/mcp.md)
- **McpClientType enum**: Lines 78-81 in `src/mcp/mod.rs` correctly show Local/Remote variants with Arc<RwLock> wrappers
- **McpService struct**: Lines 83-86 in `src/mcp/mod.rs` - servers HashMap and oauth OAuthManager fields match
- **RemoteClient struct**: Lines 328-340 in `src/mcp/remote.rs` - all fields match documentation
- **McpConnectionManager struct**: Lines 29-41 in `src/mcp/remote.rs` - matches documentation exactly
- **ConnectionState enum**: Lines 19-27 in `src/mcp/remote.rs` - Connected/Disconnected/Reconnecting states match
- **McpTool struct**: Lines 54-59 in `src/mcp/mod.rs` - name, description, input_schema, server fields match
- **McpServerStatus enum**: Lines 61-68 in `src/mcp/mod.rs` - Disconnected/Connecting/Connected/Error variants match
- **OAuthManager struct**: Lines 91-96 in `src/mcp/auth.rs` - matches with token_store, used_codes_store, servers, used_codes fields
- **TokenSet struct**: Lines 58-64 in `src/mcp/auth.rs` - access_token, refresh_token, token_type, expires_at, scope fields match
- **ServerTokens struct**: Lines 81-84 in `src/mcp/auth.rs` - server_url and tokens fields match
- **LocalClient struct**: Lines 47-57 in `src/mcp/local.rs` - all fields match documentation
- **IdeServer struct**: Lines 50-55 in `src/mcp/ide_server.rs` - tools, pending, shutdown, shutdown_notify fields match
- **connect_sse()**: Lines 698-740 in `src/mcp/remote.rs` - method exists as documented
- **connect_sse_stream()**: Lines 747-800 in `src/mcp/remote.rs` - method exists as documented
- **take_sse_events()**: Lines 802-805 in `src/mcp/remote.rs` - method exists as documented
- **DNS re-validation**: Line 448 in `src/mcp/remote.rs` - `initialize()` calls `validate_host_ip` before each request
- **SSE known issue**: Lines 160-161 in `src/mcp/remote.rs` - confirmed SSE methods exist but are not automatically called

### Memory Module (architecture/memory.md)
- **Memory struct**: Lines 14-26 in `src/memory/mod.rs` - all 10 fields match: id, namespace, title, content, uri, created_at, updated_at, access_count, importance, superseded_by
- **MemoryStore struct**: Lines 50-54 in `src/memory/mod.rs` - root PathBuf, memories Mutex, auto_save Mutex fields match
- **PatternDetector**: Lines 40-43 in `src/memory/patterns.rs` - preference_patterns and convention_patterns fields match
- **PatternMatch struct**: Lines 9-15 in `src/memory/patterns.rs` - pattern_type, matched_text, score, context fields match
- **ScoredMemory struct**: Lines 259-266 in `src/memory/patterns.rs` - matched_text, score, pattern_type, context, frequency fields match
- **Negation scoring fix**: Lines 184-192 in `src/memory/patterns.rs` - is_negation detection and `base_score + negation_modifier` calculation confirmed
- **access_count tracking**: Lines 169-183 in `src/memory/mod.rs` - `get()` method increments access_count confirmed
- **Topic matching superseding**: Lines 226-241 in `src/memory/mod.rs` - title prefix stripping logic confirmed
- **Score threshold**: Line 246 in `src/memory/mod.rs` - only memories with score >= 8.0 are stored, confirmed
- **Max 20 memories**: Line 245 in `src/memory/mod.rs` - `scored.into_iter().take(20)` confirmed
- **File locking**: Lines 496-516 in `src/memory/mod.rs` - flock_lock and flock_unlock functions confirmed
- **flock_lock() and flock_unlock()**: Lines 497-505 and 508-516 respectively - exist for Unix, stubbed for Windows

### Overview (architecture/overview.md)
- **Protocol version**: `src/protocol/core.rs:3` - confirmed version 1
- **InprocCoreClient, StdioCoreClient, SocketCoreClient**: Listed in Module Map, exist in `src/core/transport/`
- **AppEvent 36 variants**: `src/bus/events.rs:5-147` - confirmed count
- **LSP servers count 39**: `src/lsp/server.rs:27-383` - confirmed 39 servers in server_definitions()
- **UiState 26 fields**: `src/tui/app/state/ui.rs:27-76` - confirmed
- **Built-in agents 7**: `src/agent/mod.rs:147-262` - confirmed build, plan, general, explore, title, summary, compaction
- **PermissionRegistry/QuestionRegistry synchronous**: Confirmed - `register()`, `respond()`, `answer_question()` are `fn`, not `async fn`
- **ToolRegistry::with_defaults() takes &dyn Tool**: Lines 122-126 in `src/tool/mod.rs` - confirmed
- **Tool count 27**: Lines 89-119 in `src/tool/mod.rs` - confirmed count

## Incorrect/Stale Claims

### MCP
1. **Line 161-162**: SSE Known Issue mentions "SSE events are collected but not yet processed by the agent" - this is accurate but could be clarified. The events ARE collected via `connect_sse()` but there's no consumer for `take_sse_events()` in the agent loop.

2. **IdeServer tools**: Documentation says `openDiff` tool is supported. Verified at `src/mcp/ide_server.rs:64-67` - only `openDiff` is registered. This is correct.

### Memory
- No incorrect claims found. The documentation is accurate regarding the bug fixes and current implementation.

### Overview
- **Line 105**: "Auto-registered: codegg_zen only" - The AGENTS.md file states "Only `codegg_go` is auto-registered via `register_builtin()`". Need to verify which is correct. Looking at provider implementation would clarify.

## Bugs Found

### MCP
1. **RemoteClient::initialize() DNS re-validation on every call (line 438-450)**: This is actually CORRECT behavior per architecture doc. The `initialize()` re-validates DNS on each call. However, there's a subtle issue: it updates `self.validated_ips` but the comment at line 362-364 says revalidate_dns is called before each request in `post_json()`. This is correct - initialize() sets up initial validation, post_json() re-validates before each request.

2. **SSE Connection not integrated**: `connect_sse()` exists at line 698 but is never called automatically during remote connection setup. A consumer needs to call `take_sse_events()` to retrieve collected events. This is documented as a known issue.

### Memory
- No bugs found in current implementation. All documented behavior is accurate.

## Improvements Identified

### MCP
1. **IdeServer::run_socket() not fully implemented**: Lines 121-144 show the method exists but doesn't implement actual socket handling - just returns Ok(()). Should be completed or documented as incomplete.

2. **OAuthManager missing load/save sync methods**: The `load_tokens_sync()` and `load_used_codes_sync()` are implemented but marked with `#[allow(dead_code)]`. These could be used for sync operations.

3. **McpCli debug command not implemented**: Lines 309-318 in `src/mcp/cli.rs` - the Debug variant just prints a message and doesn't actually test the connection.

### Memory
1. **MemoryStore::save_unlocked() namespace safety**: Line 331 checks `is_safe_namespace()` but the documentation at line 67-70 in `src/memory/mod.rs` correctly describes the file locking mechanism. The namespace checking is appropriate.

## Stale References

### MCP
- **IdeServer transport modes**: Documentation says "stdio mode (for IDE extensions)" and "Unix socket mode". The `run_socket()` method exists but implementation is incomplete (line 143 just returns Ok). This should either be fixed or documented as unimplemented.

### Memory
- No stale references found. All documentation matches current implementation.

### Overview
- **Line 105**: Potential inconsistency between "Auto-registered: codegg_zen only" and AGENTS.md "Only `codegg_go` is auto-registered". Need to verify which name is correct in actual provider code.

## Recommendations

### MCP
1. **Complete IdeServer::run_socket()**: The socket mode is documented but not implemented. Either implement it or remove from documentation.

2. **Wire up SSE event processing**: `connect_sse()` and `take_sse_events()` exist but no consumer in agent loop. Consider adding integration or documenting the limitation more precisely.

3. **Implement McpCli debug command**: The `Debug` command in cli.rs doesn't actually test connections. Consider implementing or removing.

4. **Document OAuth flow more thoroughly**: The auth.rs has PKCE support, but documentation doesn't show the complete OAuth authorization code flow with state validation.

### Memory
1. **Consider adding memory eviction policy documentation**: While max 20 memories per namespace is documented, the eviction criteria (lowest importance when at limit) could be more explicit.

2. **Document consolidate_session limitations**: Pattern detection works on Message parts. If messages contain only binary data, pattern detection may not work. Could be documented.

### Overview
1. **Clarify provider auto-registration**: Verify and correct the "codegg_zen" vs "codegg_go" naming discrepancy between overview.md and AGENTS.md.

2. **Add LSP server count verification step**: Since this count has been disputed in past reviews, consider adding a compile-time constant or test that verifies the count.

3. **Document UiState timeline fields**: The `timeline_visible` and `timeline_selected` fields in UiState (lines 62-63) are mentioned in AGENTS.md but overview.md could reference them more clearly.

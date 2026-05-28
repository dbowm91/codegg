# Review: Batch 2 - Core Facade and Transport

**Reviewed**: 2026-05-28
**Files**: architecture/core.md, architecture/server.md, architecture/client.md, architecture/exec.md

## Summary

Reviewed four architecture documents against actual source code. Found 1 significant documentation error in exec.md regarding the question channel deadlock fix (doc describes wrong method name and wrong behavior), 1 type annotation error in core.md (pool field type), and 1 line number offset in core.md. The server and client documentation is highly accurate with all route definitions, middleware stacks, and protocol details matching the codebase.

## Documentation Issues

| # | File | Line | Issue | Action |
|---|------|------|-------|--------|
| 1 | core.md | 37 | InprocCoreClient `pool` field type incorrect: doc says "all wrapped in `Option<Arc<T>>`" but `pool` is `Option<sqlx::SqlitePool>` (not `Option<Arc<sqlx::SqlitePool>>`) per `src/core/mod.rs:27` | UPDATE |
| 2 | core.md | 137 | `map_app_event_to_core_event` location says `src/core/mod.rs:728-841` but function spans lines 733-849 (off by ~5-8 lines) | UPDATE |
| 3 | exec.md | 169 | Doc says `setup_question_channel()` is called and exec mode returns "[question not supported in exec mode]" immediately. Actual code calls `setup_question_channel_for_exec()` which DOES set `question_rx = Some(rx)`, so exec mode waits 300s before timing out. The "[question not supported]" string is in the `else` branch (when `question_rx` is None), which is the NON-exec path | UPDATE |
| 4 | exec.md | 169 | Doc does not mention `setup_question_channel_for_exec()` exists. `setup_question_channel()` (non-exec version) is defined at `src/agent/loop.rs:784` but never called anywhere in the codebase (dead code) | NEW |
| 5 | client.md | 89 | `RenderFrame` variant listed as "Frame content (legacy - received and logged, not rendered)" but server.md's Client→Server table omits it entirely. Both are technically correct but the inconsistency may confuse readers | NEW |
| 6 | ws.rs | 357 | `handle_tui` has `#[allow(dead_code)]` annotation despite being actively used in the Axum router at `http.rs:265`. Annotation is stale/misleading | NEW |

## Code Issues Found

| # | Module | Bug/Issue | Location | Severity |
|---|--------|-----------|----------|----------|
| 1 | agent/loop.rs | `setup_question_channel()` (non-exec) is defined but never called anywhere - dead code | `src/agent/loop.rs:784-786` | Low |
| 2 | server/ws.rs | `handle_tui` function has stale `#[allow(dead_code)]` attribute | `src/server/ws.rs:357` | Low |

## Improvement Opportunities

| # | Module | Opportunity | Impact |
|---|--------|-------------|--------|
| 1 | core.md | Clarify that `pool` field in `InprocCoreClient` is `Option<SqlitePool>` not `Option<Arc<SqlitePool>>`. Update the "all wrapped in `Option<Arc<T>>`" claim to be precise about each field's type | Prevents confusion for contributors implementing new core fields |
| 2 | exec.md | Document the actual exec mode question behavior: `setup_question_channel_for_exec()` sets up the channel, meaning questions will wait up to 300s then timeout. The current behavior is suboptimal for CI/CD - consider adding a short timeout (e.g., 5s) for exec mode questions | Reduces CI/CD pipeline waste from 300s waits on questions |
| 3 | exec.md | Remove or clearly mark `setup_question_channel()` as unused/dead code, and document `setup_question_channel_for_exec()` as the primary exec-mode entry point | Prevents confusion about which method to call |
| 4 | server.md | Note that `handle_tui` in ws.rs has a stale `#[allow(dead_code)]` annotation that should be removed | Code hygiene |
| 5 | core.md | Consider documenting the `_ => Ok(CoreResponse::Ack)` fallthrough pattern at `src/core/mod.rs:703` which handles all unimplemented CoreRequest variants (Initialize, Subscribe, Resume, TurnCancel, TurnSteer, AgentSelect, ModelSelect) | Helps future contributors understand the handler coverage |

## Stale Content to Prune

| # | File | Content | Reason |
|---|------|---------|--------|
| 1 | exec.md:169 | "the question tool returns '[question not supported in exec mode]' immediately without waiting, as no question channel receiver is set up" | Incorrect: exec mode DOES set up the channel and waits 300s. The "[question not supported]" string appears only when `question_rx` is None (non-exec path) |
| 2 | exec.md:169 | "`loop_instance.setup_question_channel()` is called to enable question tool handling" | Incorrect: the actual call is `loop_instance.setup_question_channel_for_exec()` at `src/exec.rs:121` |

## Verified Claims (Correct)

| # | Module | Claim | Location | Status |
|---|--------|-------|----------|--------|
| 1 | core.md | InprocCoreClient has 4 fields: subagent_pool, memory_store, bg_scheduler, pool | `src/core/mod.rs:22-28` | CONFIRMED |
| 2 | core.md | CoreRequest has 35 variants | `src/protocol/core.rs:50-175` | CONFIRMED |
| 3 | core.md | CoreEvent has 19 variants | `src/protocol/core.rs:179-272` | CONFIRMED |
| 4 | core.md | Protocol version is 1 | `src/protocol/core.rs:3` | CONFIRMED |
| 5 | core.md | map_app_event_to_core_event maps Subagent* events | `src/core/mod.rs:803-846` | CONFIRMED |
| 6 | server.md | ServerState has 5 fields (project_dir, pool, mcp_service, config, ws_rate_limiter) | `src/server/state.rs:12-19` | CONFIRMED |
| 7 | server.md | WsRateLimiter max_requests=100, window=60s | `src/server/http.rs:208` | CONFIRMED |
| 8 | server.md | Auth middleware allows requests when no token configured | `src/server/middleware/auth.rs:37-39` | CONFIRMED |
| 9 | server.md | TUI_EVENT_BUFFER_MAX = 1024 | `src/server/ws.rs:26` | CONFIRMED |
| 10 | server.md | Compression skips 401, 403, 404, 422, 500, 502, 503 | `src/server/http.rs:36` | CONFIRMED |
| 11 | server.md | CORS allows GET, POST, DELETE | `src/server/http.rs:128-132` | CONFIRMED |
| 12 | server.md | All 30 REST/WS routes match actual router | `src/server/http.rs:220-265` | CONFIRMED |
| 13 | server.md | mDNS service name `_opencode._tcp.local.` | `src/server/mdns.rs:23` | CONFIRMED |
| 14 | server.md | mDNS multicast 224.0.0.251:5353 | `src/server/mdns.rs:10-11` | CONFIRMED |
| 15 | client.md | handle_remote_event at line 805 | `src/tui/app/mod.rs:805` | CONFIRMED |
| 16 | client.md | Health check timeout 10s | `src/client/sdk.rs:40` | CONFIRMED |
| 17 | client.md | WebSocket timeout 30s, 3 retries, backoff 1s/2s/4s | `src/client/attach.rs:39-43` | CONFIRMED |
| 18 | client.md | ClientError has 5 variants | `src/error.rs:504-519` | CONFIRMED |
| 19 | exec.md | ExecInput fields match code | `src/exec.rs:11-16` | CONFIRMED |
| 20 | exec.md | ExecOutput fields match code | `src/exec.rs:19-28` | CONFIRMED |
| 21 | exec.md | All 28 error codes match classify_error | `src/exec.rs:191-260` | CONFIRMED |
| 22 | exec.md | Exit codes: 0=success, 1=failure | `src/exec.rs:279-285` | CONFIRMED |
| 23 | exec.md | mcp_service hardcoded to None | `src/exec.rs:107` | CONFIRMED |
| 24 | exec.md | Config errors returned as CONFIG_ERROR | `src/exec.rs:83` | CONFIRMED |
| 25 | server.md | ServerRuntimeError has 5 variants with correct HTTP status mapping | `src/error.rs:458-483` | CONFIRMED |
| 26 | server.md | ResyncRequired uses TuiMessage variant directly (not raw JSON) | `src/server/ws.rs:451,460,466,560` | CONFIRMED |

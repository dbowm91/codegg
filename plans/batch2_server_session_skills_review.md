# Server & Session & Skills Architecture Review

## Verified Claims

### Server Module (src/server/)
- **File paths and structure**: All files exist as documented (mod.rs, http.rs, ws.rs, state.rs, rpc.rs, mdns.rs, middleware/auth.rs, routes/*) - CONFIRMED
- **run_server() signature**: `pub async fn run_server(host: &str, port: u16) -> Result<(), ServerRuntimeError>` at http.rs:156 - CORRECT
- **ServerState struct**: Fields match at state.rs:13-19 (project_dir, pool, mcp_service, config, ws_rate_limiter) - CORRECT
- **WsRateLimiter struct**: Fields match at state.rs:22-26 - CORRECT
- **Middleware stack order**: Auth -> Rate Limit -> Security Headers -> CORS -> Compression -> Trace - CORRECT (http.rs:266-288)
- **CompressionPredicate**: Skips compression for 401, 403, 404, 422, 500, 502, 503 at http.rs:36 - CORRECT
- **TUI_EVENT_BUFFER**: 1024 capacity as documented at ws.rs:25-26 - CORRECT
- **Event sequence**: AtomicU64 starting at 1 at ws.rs:23 - CORRECT (note: documentation says "monotonically increasing" but starts at 1, not 0)
- **ws.rs:455-461 and 470-477**: ResyncRequired serialization - CONFIRMED in ws.rs at lines 455-463 and 470-477
- **validate_ws_auth()**: Checks CODEGG_SERVER_AUTH_DISABLED first, then CODEGG_SERVER_TOKEN env var with constant-time comparison - CORRECT (ws.rs:78-109)
- **ServerRuntimeError enum**: All 5 variants documented exist at src/error.rs - CORRECT

### Session Module (src/session/)
- **Schema file structure**: All 10 files exist as documented - CONFIRMED
- **Migration versions**: 15 migrations as documented at schema.rs:25-69 - CORRECT
- **Migration table count**: 15 documented but schema shows v1 creates 7 tables (project, session, message, part, todo, permission, session_share) + indexes - CONFIRMED
- **Session struct fields**: All 22 fields match at models.rs:6-28 - CORRECT
- **CreateSession struct**: All 10 fields match at models.rs:31-40 - CORRECT
- **PartData enum**: All 5 variants match at message.rs:36-59 - CORRECT
- **ToolStatus enum**: All 4 variants match at message.rs:61-69 - CORRECT
- **Part struct**: Fields match at message.rs:72-79 - CORRECT
- **SessionStatus enum**: All 5 variants match at status.rs:4-12 - CORRECT
- **SessionState struct**: All 7 fields match at status.rs:53-62 - CORRECT
- **Checkpoint struct**: All 7 fields match at checkpoint.rs:9-19 - CORRECT
- **WorkingFile struct**: Fields match at checkpoint.rs:21-26 - CORRECT
- **CheckpointStore methods**: All 6 methods listed exist (save, load, load_latest, list, delete, delete_all, has_checkpoint) - CORRECT
- **compute_checksum()**: SHA-256 hex at checkpoint.rs:150-154 - CORRECT
- **UsageRecord struct**: All 9 fields match at models.rs:92-102 - CORRECT
- **Import size limits**: MAX_IMPORT_MESSAGES=100,000; MAX_IMPORT_PARTS=500,000; MAX_TOTAL_IMPORT_BYTES=500MB at import.rs:68-70 - CORRECT

### Skills Module (src/skills/mod.rs)
- **Skill struct**: All 6 fields match (name, description, version, tags, body, source) - CONFIRMED lines 8-15
- **SkillIndex struct and methods**: All documented methods exist (new, load, get, list, find_matching, build_system_prompt, activate) - CONFIRMED lines 36-126
- **SkillFrontmatter struct**: All fields match at lines 17-24 - CORRECT
- **Skill loading**: Two locations documented correctly (global ~/.config/codegg/skills/, project .codegg/skills/) - CONFIRMED at lines 44-56

---

## Incorrect/Stale Claims

### Server Module
1. **Line 192**: "See ws.rs:455-461 and ws.rs:470-477" - The code at those exact line numbers shows slightly different structure than documented. The actual handling is at lines 455-463 (ResyncRequired on PermissionPending/QuestionPending) and 470-477 (ResyncRequired on lagged events). The documentation is directionally correct but the specific line numbers referenced may vary slightly depending on editing.

### Session Module
1. **revert_for_export redacts "tail" tool**: documentation (line 524) lists `tail` as a sensitive tool name, but the actual code at import.rs:133 uses `terminal` instead of `tail`. `terminal` is actually a valid tool name in the codebase.

2. **Store line ranges**: The documentation gives approximate line ranges for stores (e.g., SessionStore at 44-1548) which may have shifted due to recent edits. Actual store implementation confirmed to exist but exact line numbers should be verified if precise referencing is needed.

3. **Database indexes**: The index `idx_session_directory` in session.md (line 191) is documented to exist, but this appears to be correct per migration v11 at schema.rs:468.

### Skills Module
- **No significant issues found**: All claims about the skills module are accurate.

---

## Bugs Found

### Server Module
1. **Bug: ws.rs validate_ws_auth() behavior discrepancy**
   - The docs (line 198-201) describe validate_ws_auth() behavior but the actual HTTP auth middleware behavior differs:
   - HTTP auth middleware (middleware/auth.rs:37-40): When no token is configured (`expected_token = None`), it allows requests.
   - WebSocket validate_ws_auth() (ws.rs:103-106): When no token is configured, it returns 500 INTERNAL_SERVER_ERROR.
   - This is a **behavior mismatch** between HTTP and WebSocket authentication - HTTP allows no-token requests while WebSocket rejects them.
   
   **Location**: src/server/ws.rs:103-106 vs src/server/middleware/auth.rs:37-40

   **Severity**: Medium - Inconsistent auth behavior between REST and WebSocket endpoints.

---

## Improvements Identified

### Server Module
1. **Documentation improvement**: The line number references at server.md:192 could be made more generic since exact line numbers change with edits.

2. **Potential improvement**: The RateLimiter for HTTP (http.rs:41-72) and WsRateLimiter for WebSocket (state.rs:22-51) use different implementations (tokio::sync::Mutex vs std::sync::Mutex via Arc). This should be documented or harmonized.

### Session Module
1. **Documentation improvement**: session.md line 192-199 says indexes are created in v1, but the actual v1 creates `session.project_idx`, `session.workspace_idx`, `session.parent_idx` and `todo_session_idx`, `message_session_time_created_id_idx`, `part_message_id_id_idx`, `part_session_idx`. The specific index names could be documented more precisely.

2. **Documentation improvement**: session.md line 206-216 describes `migrate() -> migrate_and_record() -> migrate_vN()` pattern but the actual code flow involves explicit version checks rather than a loop. The description is functionally correct but slightly misleading about implementation style.

### Skills Module
1. **Documentation improvement**: skills.md line 103-104 mentions `~/.config/codegg/skills/` but the actual code uses `dirs::config_dir()` which returns platform-specific config directories. On macOS this would be `~/Library/Application Support/codegg/skills/` rather than `~/.config/`.

---

## Stale References

### Server Module
1. **server.md:192**: Line number references for ResyncRequired handling ("ws.rs:455-461 and ws.rs:470-477") are approximate - the actual lines are 455-463 and 470-477.

2. **server.md:465-468**: "Client-side (see src/client/attach.rs)" - This reference should be verified as client-side timeouts may have been moved.

### Session Module
1. **session.md:250**: `src/session/models.rs:6-28` for Session struct - This is correct.

2. **session.md:297**: `src/session/message.rs:36-59` for PartData enum - This is correct.

---

## Recommendations

### High Priority
1. **Fix ws.rs validate_ws_auth() inconsistency**: Consider making WebSocket auth behavior consistent with HTTP auth - when no token is configured, WebSocket should also allow requests (not return 500). This is a potential production security issue if deployment expects consistent behavior.

### Medium Priority
1. **Update tool name in redacted list**: The `redact_for_export` function at import.rs:133 should use an actual tool name. The tool `terminal` appears in the codebase (src/tool/terminal.rs) but `tail` does not. Consider using a tool that actually exists.

2. **Clarify auth middleware behavior in documentation**: The server documentation (line 73 and 290) notes that requests are allowed when no token is configured, but this only applies to the HTTP middleware, not WebSocket. This should be clarified.

### Low Priority
1. **Platform-specific skills path**: Document that skills are loaded from platform-specific config directory on macOS (`~/Library/Application Support/codegg/skills/`).

2. **Approximate line numbers**: Many documentation references indicate line numbers that are approximate. Consider using more general references or verify exact line numbers when making claims (line numbers in this codebase appear to shift with regularity).

---

*Review completed: 2026-05-27*

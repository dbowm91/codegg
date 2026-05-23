# Session Module Architecture Review

## Verification Results

### Claims

| Claim | Status | Evidence |
|-------|--------|----------|
| Session struct with all 22 fields (id, project_id, workspace_id, parent_id, slug, directory, title, version, share_url, summary_additions, summary_deletions, summary_files, summary_diffs, revert, permission, tags, time_created, time_updated, time_compacting, time_archived, time_deleted) | VERIFIED | `src/session/models.rs:6-28` matches exactly |
| Message struct stored as JSON with id, session_id, time_created, time_updated, data | VERIFIED | `src/session/message.rs:3-10` |
| MessageData with id, session_id, message_id (renamed), parts | VERIFIED | `src/session/message.rs:12-23` |
| PartInfo with id, session_id, message_id (renamed), data (flattened) | VERIFIED | `src/session/message.rs:25-34` |
| PartData enum with Text, Reasoning, ToolCall, Image, File variants | VERIFIED | `src/session/message.rs:36-59` |
| ToolStatus enum with Pending (default), Running, Completed, Error | VERIFIED | `src/session/message.rs:61-69` |
| Checkpoint struct with id, timestamp, session_id, provider, model, messages, completed_steps, working_files | VERIFIED | `src/session/checkpoint.rs:9-19` |
| WorkingFile struct with path, checksum, pre_state | VERIFIED | `src/session/checkpoint.rs:21-26` |
| TodoItem struct with all 7 fields | VERIFIED | `src/session/models.rs:65-74` |
| SessionStatus enum with Idle (default), Busy, Error, Compacting, Exporting | VERIFIED | `src/session/status.rs:4-12` |
| SessionState struct with status, started_at, last_activity, turn_count, token_in, token_out, error_message | VERIFIED | `src/session/status.rs:53-62` |
| SessionStore::create() takes CreateSession, returns Session | VERIFIED | `src/session/store.rs:57-128` |
| SessionStore::create_from_template() exists | VERIFIED | `src/session/store.rs:130-147` |
| SessionStore::update() takes id and UpdateSession | VERIFIED | `src/session/store.rs:637-682` |
| SessionStore::delete() is soft delete | VERIFIED | `src/session/store.rs:684-687` calls soft_delete |
| SessionStore::list() with limit | VERIFIED | `src/session/store.rs:161-163` |
| SessionStore::list_with_offset() exists | VERIFIED | `src/session/store.rs:165-182` |
| SessionStore::list_all() exists | VERIFIED | `src/session/store.rs:231-237` |
| SessionStore::list_all_with_offset() exists | VERIFIED | `src/session/store.rs:239-270` |
| SessionStore::search() exists | VERIFIED | `src/session/store.rs:272-291` |
| SessionStore::search_all() exists | VERIFIED | `src/session/store.rs:293-313` |
| SessionStore::find_by_tag() exists | VERIFIED | `src/session/store.rs:315-332` |
| SessionStore::all_tags() exists | VERIFIED | `src/session/store.rs:334-355` |
| SessionStore::session_count() exists | VERIFIED | `src/session/store.rs:184-193` |
| SessionStore::message_count() exists | VERIFIED | `src/session/store.rs:195-202` |
| SessionStore::message_counts() exists | VERIFIED | `src/session/store.rs:204-229` |
| SessionStore::soft_delete() exists | VERIFIED | `src/session/store.rs:689-703` |
| SessionStore::restore() exists | VERIFIED | `src/session/store.rs:705-718` |
| SessionStore::list_deleted() exists | VERIFIED | `src/session/store.rs:720-730` |
| SessionStore::archive() exists | VERIFIED | `src/session/store.rs:962-976` |
| SessionStore::unarchive() exists | VERIFIED | `src/session/store.rs:978-991` |
| SessionStore::fork() exists | VERIFIED | `src/session/store.rs:750-960` |
| SessionStore::children() exists | VERIFIED | `src/session/store.rs:1009-1019` |
| SessionStore::set_tags() exists | VERIFIED | `src/session/store.rs:732-748` |
| SessionStore::revert_to_message() exists | VERIFIED | `src/session/store.rs:1021-1170` |
| SessionStore::unrevert_session() exists | VERIFIED | `src/session/store.rs:1363-1493` |
| SessionStore::share_session() exists | VERIFIED | `src/session/store.rs:1266-1330` |
| SessionStore::unshare_session() exists | VERIFIED | `src/session/store.rs:1332-1361` |
| SessionStore::set_share_url() exists | VERIFIED | `src/session/store.rs:993-1007` |
| SessionStore::generate_summary() exists | VERIFIED | `src/session/store.rs:1172-1202` |
| SessionStore::generate_title() exists | VERIFIED | `src/session/store.rs:1204-1234` |
| SessionStore::export_session() exists | VERIFIED | `src/session/store.rs:357-436` |
| SessionStore::import_session() exists | VERIFIED | `src/session/store.rs:438-635` |
| SessionStore::get_analytics() exists | VERIFIED | `src/session/store.rs:1495-1547` |
| CheckpointStore::new(pool) exists | VERIFIED | `src/session/checkpoint.rs:48-51` |
| CheckpointStore::save() exists | VERIFIED | `src/session/checkpoint.rs:53-72` |
| CheckpointStore::load() exists | VERIFIED | `src/session/checkpoint.rs:74-88` |
| CheckpointStore::load_latest() exists | VERIFIED | `src/session/checkpoint.rs:90-106` |
| CheckpointStore::list() exists | VERIFIED | `src/session/checkpoint.rs:108-124` |
| CheckpointStore::delete() exists | VERIFIED | `src/session/checkpoint.rs:126-133` |
| CheckpointStore::delete_all() exists | VERIFIED | `src/session/checkpoint.rs:135-142` |
| CheckpointStore::has_checkpoint() exists (renamed from has_unfinished) | VERIFIED | `src/session/checkpoint.rs:144-147` |
| compute_checksum() helper exists | VERIFIED | `src/session/checkpoint.rs:150-154` |
| create_working_file() helper exists | VERIFIED | `src/session/checkpoint.rs:156-166` |
| verify_file() helper exists | VERIFIED | `src/session/checkpoint.rs:168-177` |
| validate_import_size() enforces limits | VERIFIED | `src/session/import.rs:72-105` |
| MAX_IMPORT_MESSAGES=100,000 | VERIFIED | `src/session/import.rs:68` |
| MAX_IMPORT_PARTS=500,000 | VERIFIED | `src/session/import.rs:69` |
| MAX_TOTAL_IMPORT_BYTES=500MB | VERIFIED | `src/session/import.rs:70` |
| redact_for_export() exists | VERIFIED | `src/session/import.rs:107-180` |
| redacts bash, write, read, edit, replace, multiedit, terminal, git, webfetch, apply_patch | VERIFIED | `src/session/import.rs:127-136` |
| Database schema v1-v14 defined | VERIFIED | `src/session/schema.rs:1-513` |
| session table matches schema | VERIFIED | `src/session/schema.rs:143-171` |
| message table matches schema | VERIFIED | `src/session/schema.rs:173-187` |
| part table matches schema | VERIFIED | `src/session/schema.rs:189-204` |
| todo table matches schema | VERIFIED | `src/session/schema.rs:206-223` |
| session_share table exists | VERIFIED | `src/session/schema.rs:240-255` |
| share_expires_at added in v5 | VERIFIED | `src/session/schema.rs:347-354` |
| task table (v9) matches schema | VERIFIED | `src/session/schema.rs:404-436` |
| snapshot table (v13) matches schema | VERIFIED | `src/session/schema.rs:481-504` |
| migration_version table exists | VERIFIED | `src/session/schema.rs:6-16` |
| cached_models table (v3) exists | VERIFIED | `src/session/schema.rs:311-336` |
| checkpoint table (v10) exists | VERIFIED | `src/session/schema.rs:439-461` |
| SessionCreated event published | VERIFIED | `src/bus/events.rs:7` |
| MessageAdded event published | VERIFIED | `src/bus/events.rs:21` |
| escape_sql_like() helper exists | VERIFIED | `src/session/store.rs:22-26` |
| generate_slug() helper exists | VERIFIED | `src/session/store.rs:28-42` |
| parse_json_field() helper exists | VERIFIED | `src/session/mod.rs:48-64` |
| SESSION_COLUMNS constant exists | VERIFIED | `src/session/mod.rs:30-33` |
| SESSION_COLUMNS_QUALIFIED constant exists | VERIFIED | `src/session/mod.rs:35-38` |
| MESSAGE_QUERY constant exists | VERIFIED | `src/session/mod.rs:40-42` |
| PART_QUERY constant exists | VERIFIED | `src/session/mod.rs:44-46` |
| Module exports match (SessionStore, MessageStore, PartStore, etc.) | VERIFIED | `src/session/mod.rs:20-28` |
| CreateSession has agent and model fields (not stored) | VERIFIED | `src/session/models.rs:31-40` agent/model accepted but not in session table |
| CheckpointStore::has_checkpoint() renamed from has_unfinished() | VERIFIED | `src/session/checkpoint.rs:144` confirmed |
| PartRow uses parse_json_field() while MessageRow uses TryFrom | VERIFIED | Known inconsistency noted at `row.rs:77-86` vs `row.rs:99-110` |
| SessionStatus and SessionState are for TUI display | VERIFIED | Both in `src/session/status.rs` |
| TodoStore exists with list, set, add, update, remove, clear | VERIFIED | `src/session/store.rs:1550-1753` |
| MessageStore exists with create, get, list, count, update, delete | VERIFIED | `src/session/store.rs:1755-1878` |
| PartStore exists with create, get, list_by_message, list_by_session, update, delete | VERIFIED | `src/session/store.rs:1880-1996` |
| PermissionStore exists with get, upsert, delete | VERIFIED | `src/session/store.rs:1998-2061` |

### Claims NOT in Architecture but in Code

| Claim | Status | Evidence |
|-------|--------|----------|
| SessionStore::pool() method returns SqlitePool clone | VERIFIED | `src/session/store.rs:53-55` |
| CheckpointStore::pool() not exposed | N/A - private | Internal only |
| PartStore and PermissionStore have new()/pool() | VERIFIED | Each store has `pub fn new(pool: SqlitePool)` |
| MessageStore has pool() | N/A - not in any export list | Private method |
| get_conversation_text() private helper exists | VERIFIED | `src/session/store.rs:1236-1264` |
| task table has allowed_paths column added in v14 | VERIFIED | `src/session/schema.rs:506-512` |

## Bugs Found

### Medium

1. **create_working_file() uses blocking I/O in async context**
   - `src/session/checkpoint.rs:156-166` calls `std::fs::read_to_string()` directly
   - This blocks the async runtime thread
   - Should use `tokio::fs::read_to_string()` instead

2. **verify_file() uses blocking I/O**
   - `src/session/checkpoint.rs:168-177` uses `std::fs::read_to_string()`
   - Same issue as above - blocks async thread

### Low

3. **PartStore NotFound error message says "session" instead of "part"**
   - `src/session/store.rs:1918` - Error says `format!("session {id}")` but should say `format!("part {id}")`
   - Minor copy-paste error

4. **fork() and revert_to_message() don't clear time_compacting field**
   - `fork()` creates a new session but doesn't reset `time_compacting` (line 956 sets to None, OK)
   - Actually looking again at line 956, it does set `time_compacting: None` which is correct
   - But `unrevert_session()` at line 1491 also sets `revert = None` which is correct
   - No bug here, just verifying

## Improvement Suggestions

### Correctness

1. **Consider adding database transaction rollback on error in unrevert_session()**
   - Currently `unrevert_session()` returns early on error without explicit rollback
   - The transaction will auto-rollback when dropped, but explicit error handling would be clearer
   - However SQLx handles this automatically, so low priority

2. **Consider validating message_id exists when importing parts**
   - `import_session()` at line 535-538 uses `unwrap_or(old_msg_id)` when message_id mapping not found
   - Could silently create orphan parts if import data is malformed
   - Could add validation or `EXPECT` with clear error

### Performance

1. **all_tags() loads all session rows to count tags**
   - `src/session/store.rs:334-355` loads full rows just to extract tags
   - Could use direct SQL query with JSON extraction
   - For large session counts, this is inefficient

2. **fork() loads all messages and parts into memory before bulk insert**
   - `src/session/store.rs:791-819` iterates all messages to build redacted copies
   - For sessions with many messages, this is memory-intensive
   - Consider streaming/batch processing for very large sessions

3. **import_session() could batch insert messages/parts more efficiently**
   - Current implementation builds Vec then uses QueryBuilder - this is already fairly efficient
   - For extremely large imports, could consider chunked inserts

### Maintainability

1. **Add missing Error variant in import validation**
   - `validate_import_size()` returns `StorageError::Import` for size violations
   - But import_session() could fail with generic Database errors too
   - Consider adding specific import-related error variants

2. **Consider adding test coverage for checkpoint.rs helpers**
   - `compute_checksum()`, `create_working_file()`, `verify_file()` have no tests
   - Adding tests would improve confidence in these helpers

3. **Consider consolidating JSON parsing patterns**
   - `MessageRow::try_from()` uses strict `serde_json::from_str()` and returns error
   - `PartRow::from()` uses `parse_json_field()` which warns and returns Null
   - This inconsistency is documented but could be unified

4. **Consider extracting message_id validation to a reusable function**
   - The `unwrap_or(msg_id)` pattern in unrevert_session() and import_session() is repeated
   - Could extract to helper for consistency

## Priority Actions (top 5 items to fix)

1. **Medium - Fix create_working_file() and verify_file() blocking I/O**
   - Convert to `tokio::fs` for proper async runtime integration
   - Affects checkpoint operations during session save/resume

2. **Medium - Fix PartStore NotFound error message**
   - Change "session {id}" to "part {id}" at line 1918
   - Trivial one-line fix

3. **Low - Add validation for orphan parts in import_session()**
   - When message_id mapping not found, currently uses old ID silently
   - Consider logging warning or returning explicit error

4. **Low - Add test coverage for checkpoint helpers**
   - `compute_checksum()`, `create_working_file()`, `verify_file()` need tests
   - Improves maintainability and refactoring confidence

5. **Low - Consider all_tags() optimization**
   - Currently loads all sessions into memory just to count tags
   - Could be optimized with JSON SQL functions if performance becomes an issue
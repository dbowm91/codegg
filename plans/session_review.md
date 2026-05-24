# Session Module Architecture Review

**Date:** 2026-05-24  
**Reviewer:** File Search Specialist  
**Files Reviewed:**
- `architecture/session.md`
- `src/session/mod.rs`
- `src/session/models.rs`
- `src/session/store.rs`
- `src/session/checkpoint.rs`
- `src/session/import.rs`
- `src/session/message.rs`
- `src/session/row.rs`
- `src/session/schema.rs`
- `src/session/status.rs`

## Summary

The architecture document at `architecture/session.md` is **largely accurate** and reflects the actual implementation in `src/session/`. Most types, methods, database schema, and exports match between the documentation and code. However, there are some minor inconsistencies noted below.

---

## Verified Correct Items

### 1. Key Types (Session, Message, Checkpoint, etc.)
- **Session struct** (`models.rs:6-28`) matches the documented struct exactly with all 19 fields
- **Message/MessageData/PartInfo/PartData** (`message.rs`) match documented types
- **Checkpoint/WorkingFile** (`checkpoint.rs:10-26`) match documented types
- **TodoItem** (`models.rs:66-74`) matches documented type
- **SessionStatus** (`status.rs:5-12`) and **SessionState** (`status.rs:54-62`) match documented types

### 2. Store Methods
Most `SessionStore`, `TodoStore`, `MessageStore`, `PartStore`, and `PermissionStore` methods match the documentation:
- Session CRUD: `create`, `get`, `update`, `delete` ✓
- Listing/search: `list`, `list_with_offset`, `list_all`, `list_all_with_offset`, `search`, `search_all`, `find_by_tag`, `all_tags` ✓
- Soft delete/restore: `soft_delete`, `restore`, `list_deleted` ✓
- Archive: `archive`, `unarchive` ✓
- Fork: `fork` ✓
- Children: `children` ✓
- Tags: `set_tags` ✓
- Revert: `revert_to_message`, `unrevert_session` ✓
- Sharing: `share_session`, `unshare_session`, `set_share_url` ✓
- Import/export: `export_session`, `import_session` ✓
- Analytics: `get_analytics` ✓

### 3. CheckpointStore Methods
All documented methods match (`checkpoint.rs`):
- `new`, `save`, `load`, `load_latest`, `list`, `delete`, `delete_all`, `has_checkpoint` ✓
- Helper functions `compute_checksum`, `create_working_file`, `verify_file` ✓

### 4. Database Schema
All tables documented in `architecture/session.md` are correctly implemented in `schema.rs`:
- `session`, `message`, `part`, `todo`, `checkpoints`, `session_share`, `task`, `snapshot`, `cached_models`, `migration_version` ✓
- Migration versions v1-v14 correctly implemented ✓
- `session_share` has `share_expires_at` column added in v5 ✓
- `task` has `allowed_paths` column added in v14 ✓

### 5. Module Exports
`mod.rs` exports match the documented exports:
```rust
pub use models::{CreateSession, PermissionEntry, Session, SessionAnalytics, SessionSummaryProvider, TodoItem, TodoItemInput, UpdateSession};
pub use row::{MessageRow, PartRow, PermissionRow, SessionRow, TodoRow};
pub use store::{escape_sql_like, generate_slug, MessageStore, PartStore, PermissionStore, SessionStore, TodoStore};
pub use checkpoint::CheckpointStore;
```

### 6. Query Constants
`SESSION_COLUMNS`, `SESSION_COLUMNS_QUALIFIED`, `MESSAGE_QUERY`, `PART_QUERY` all match between `mod.rs` and documentation ✓

### 7. Event Publishing
Only `SessionCreated` and `MessageAdded` events exist (line 471 correctly notes `SessionSelected`, `SessionDeleted`, `SessionRenamed` are NOT published) ✓

---

## Discrepancies Found

### 1. Minor: `PartRow` JSON Parsing Inconsistency (Known Issue)
**Location:** `architecture/session.md:508`  
**Status:** Correctly documented as a known issue

The architecture doc notes (line 508):
> `PartRow` uses `parse_json_field()` helper while `MessageRow` uses direct `TryFrom` deserialization - this inconsistency in JSON error handling is known

This is confirmed:
- `MessageRow::try_into()` (`row.rs:76-86`) returns error on JSON parse failure
- `PartRow::into()` (`row.rs:99-110`) uses `parse_json_field()` which logs warning and returns `Null` on failure

This is a design choice, not a bug.

### 2. Minor: `CheckpointStore::has_checkpoint()` Renamed (Already Updated)
**Location:** `architecture/session.md:507`  
**Status:** Correct - was renamed from `has_unfinished()`

The architecture doc correctly notes the rename from `has_unfinished()` to `has_checkpoint()`. The actual implementation at `checkpoint.rs:144-147` uses `has_checkpoint`.

### 3. Minor: `create_working_file` Returns `Option<WorkingFile>`
**Location:** `architecture/session.md:287`  
**Implementation:** `checkpoint.rs:156-166`

The function can return `None` if file cannot be read (uses `std::fs::read_to_string(&path).ok()?`), which is correctly not shown in the architecture doc signature.

---

## Issues Found in Code

### 1. `get_analytics` uses hardcoded `part_type = 'tool_call'` string
**Location:** `store.rs:1522`

```rust
AND p.part_type = 'tool_call'
```

This is a SQLite generated column (defined in migration v8):
```rust
part_type TEXT GENERATED ALWAYS AS (json_extract(data, '$.type')) STORED
```

The string comparison `'tool_call'` relies on the JSON type value being exactly that string. This is fragile but not a bug per se - it works if all tool_call parts have `{"type": "tool_call", ...}` format.

### 2. `session_share` table missing from `session_share` row type
**Location:** `row.rs`

There is no `SessionShareRow` struct to map the `session_share` table. However, this is likely intentional as `share_session`/`unshare_session` directly use raw SQL without a row struct.

### 3. Error Handling in `PartRow::from()` is Silent
**Location:** `row.rs:99-110`

```rust
impl From<PartRow> for message::Part {
    fn from(r: PartRow) -> Self {
        Self {
            ...
            data: super::parse_json_field(&r.data),  // Returns Null on error, no error propagated
        }
    }
}
```

Unlike `MessageRow::try_into()` which properly returns errors, `PartRow::from()` silently converts parse failures to `Null`. This is the inconsistency mentioned in the architecture doc (line 508).

---

## Recommendations

### 1. Documentation - No Changes Needed
The architecture document is accurate and well-maintained. Only minor improvements possible:

- Consider documenting that `create_working_file()` returns `Option<WorkingFile>` and can fail silently if file cannot be read
- Consider marking the `PartRow` inconsistency as a potential future improvement (not just a known issue)

### 2. Code - Consider Adding SessionShareRow
If `SessionShareRow` functionality is needed, consider adding a row type for the `session_share` table for consistency.

### 3. Code - Consider Unifying JSON Error Handling
Consider having `PartRow::from()` use `TryFrom` like `MessageRow` for consistent error handling, though this would be a breaking change.

---

## Skill File Status

The skill file at `.opencode/skills/session/**/*` does not exist (no files found). However, the architecture document itself is comprehensive and serves as the primary documentation.

---

## Conclusion

The session module architecture and implementation are well-aligned. The architecture document at `architecture/session.md` accurately reflects the actual implementation with only minor known inconsistencies documented. No critical bugs were found. The code is production-quality with proper error handling, transaction support, and comprehensive database schema management.

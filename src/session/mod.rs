//! Session management and persistence.
//!
//! This module handles session storage, retrieval, and management. Sessions track
//! conversation history, metadata, and analytics. Sessions can be archived and
//! resumed across CLI invocations.

pub mod checkpoint;
pub mod message;
pub mod schema;
pub mod status;

pub mod import;
pub mod models;
pub mod row;
pub mod store;

use serde_json;
use tracing::warn;

pub use models::{
    CreateSession, PermissionEntry, Session, SessionAnalytics, SessionSummaryProvider, TodoItem,
    TodoItemInput, UpdateSession, UsageRecord,
};
pub use row::{MessageRow, PartRow, PermissionRow, SessionRow, TodoRow};
pub use store::{
    escape_sql_like, generate_slug, MessageStore, PartStore, PermissionStore, SessionStore, TodoStore,
    UsageStore,
};
pub use checkpoint::{
    CheckpointStore, compute_checksum, create_working_file, verify_file,
};

const SESSION_COLUMNS: &str = r#"id, project_id, workspace_id, parent_id, slug, directory,
    title, version, share_url, summary_additions, summary_deletions,
    summary_files, summary_diffs, revert, permission, tags,
    time_created, time_updated, time_compacting, time_archived, time_deleted"#;

const SESSION_COLUMNS_QUALIFIED: &str = r#"s.id, s.project_id, s.workspace_id, s.parent_id, s.slug, s.directory,
    s.title, s.version, s.share_url, s.summary_additions, s.summary_deletions,
    s.summary_files, s.summary_diffs, s.revert, s.permission, s.tags,
    s.time_created, s.time_updated, s.time_compacting, s.time_archived, s.time_deleted"#;

const MESSAGE_QUERY: &str = r#"SELECT id, session_id, time_created, time_updated, data
    FROM message WHERE session_id = ?
    ORDER BY time_created ASC, id ASC"#;

const PART_QUERY: &str = r#"SELECT id, message_id, session_id, time_created, time_updated, data
    FROM part WHERE session_id = ?
    ORDER BY time_created ASC, id ASC"#;

pub(crate) fn parse_json_field(raw: &str) -> serde_json::Value {
    match serde_json::from_str(raw) {
        Ok(v) => v,
        Err(e) => {
            let preview = if raw.len() > 100 {
                format!("{}...", &raw[..100])
            } else {
                raw.to_string()
            };
            warn!(
                "failed to parse JSON field (input preview: {}): {}",
                preview, e
            );
            serde_json::Value::Null
        }
    }
}

pub(crate) use import::redact_for_export;

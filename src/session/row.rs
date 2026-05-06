//! Database row types and From conversions for session module.

use super::message;
use super::models::{PermissionEntry, Session, TodoItem};
use crate::error::StorageError;

#[derive(sqlx::FromRow)]
pub struct SessionRow {
    pub id: String,
    pub project_id: String,
    pub workspace_id: Option<String>,
    pub parent_id: Option<String>,
    pub slug: String,
    pub directory: String,
    pub title: String,
    pub version: String,
    pub share_url: Option<String>,
    pub summary_additions: Option<i64>,
    pub summary_deletions: Option<i64>,
    pub summary_files: Option<i64>,
    pub summary_diffs: Option<String>,
    pub revert: Option<String>,
    pub permission: Option<String>,
    pub tags: Option<String>,
    pub time_created: i64,
    pub time_updated: i64,
    pub time_compacting: Option<i64>,
    pub time_archived: Option<i64>,
    pub time_deleted: Option<i64>,
}

impl From<SessionRow> for Session {
    fn from(r: SessionRow) -> Self {
        let tags: Vec<String> = r
            .tags
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default();
        Self {
            id: r.id,
            project_id: r.project_id,
            workspace_id: r.workspace_id,
            parent_id: r.parent_id,
            slug: r.slug,
            directory: r.directory,
            title: r.title,
            version: r.version,
            share_url: r.share_url,
            summary_additions: r.summary_additions,
            summary_deletions: r.summary_deletions,
            summary_files: r.summary_files,
            summary_diffs: r.summary_diffs.and_then(|s| serde_json::from_str(&s).ok()),
            revert: r.revert.and_then(|s| serde_json::from_str(&s).ok()),
            permission: r.permission.and_then(|s| serde_json::from_str(&s).ok()),
            tags,
            time_created: r.time_created,
            time_updated: r.time_updated,
            time_compacting: r.time_compacting,
            time_archived: r.time_archived,
            time_deleted: r.time_deleted,
        }
    }
}

#[derive(sqlx::FromRow)]
pub struct MessageRow {
    pub id: String,
    pub session_id: String,
    pub time_created: i64,
    pub time_updated: i64,
    pub data: String,
}

impl TryFrom<MessageRow> for message::Message {
    type Error = StorageError;

    fn try_from(r: MessageRow) -> Result<Self, Self::Error> {
        let data: message::MessageData =
            serde_json::from_str(&r.data).map_err(|e| StorageError::Database(e.to_string()))?;
        Ok(Self {
            id: r.id,
            session_id: r.session_id,
            time_created: r.time_created,
            time_updated: r.time_updated,
            data,
        })
    }
}

#[derive(sqlx::FromRow)]
pub struct PartRow {
    pub id: String,
    pub message_id: String,
    pub session_id: String,
    pub time_created: i64,
    pub time_updated: i64,
    pub data: String,
}

impl From<PartRow> for message::Part {
    fn from(r: PartRow) -> Self {
        Self {
            id: r.id,
            message_id: r.message_id,
            session_id: r.session_id,
            time_created: r.time_created,
            time_updated: r.time_updated,
            data: super::parse_json_field(&r.data),
        }
    }
}

#[derive(sqlx::FromRow)]
pub struct TodoRow {
    pub session_id: String,
    pub content: String,
    pub status: String,
    pub priority: String,
    pub position: i64,
    pub time_created: i64,
    pub time_updated: i64,
}

impl From<TodoRow> for TodoItem {
    fn from(r: TodoRow) -> Self {
        Self {
            session_id: r.session_id,
            content: r.content,
            status: r.status,
            priority: r.priority,
            position: r.position,
            time_created: r.time_created,
            time_updated: r.time_updated,
        }
    }
}

#[derive(sqlx::FromRow)]
pub struct PermissionRow {
    pub project_id: String,
    pub time_created: i64,
    pub time_updated: i64,
    pub data: String,
}

impl From<PermissionRow> for PermissionEntry {
    fn from(r: PermissionRow) -> Self {
        Self {
            project_id: r.project_id,
            time_created: r.time_created,
            time_updated: r.time_updated,
            data: super::parse_json_field(&r.data),
        }
    }
}

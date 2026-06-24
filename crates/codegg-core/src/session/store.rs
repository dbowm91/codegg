//! Store implementations for session, todo, message, part, and permission.

use chrono::Utc;
use rand::RngCore;
use sqlx::{query_builder::QueryBuilder, SqlitePool};
use std::collections::HashMap;
use uuid::Uuid;

use super::import::{validate_import_size, SessionImport};
use super::message;
use super::models::{
    CreateSession, PermissionEntry, Session, SessionAnalytics, TodoItem, TodoItemInput,
    UpdateSession, UsageRecord,
};
use super::row::{MessageRow, PartRow, SessionRow, TodoRow};
use super::{
    parse_json_field, MESSAGE_QUERY, PART_QUERY, SESSION_COLUMNS, SESSION_COLUMNS_QUALIFIED,
};
use crate::error::StorageError;
use codegg_config::schema::SessionTemplate;

/// Row type returned by usage queries (id, session_id, provider, model,
/// input_tokens, output_tokens, cached_tokens, cost_usd, timestamp).
type UsageRow = (String, String, String, String, i64, i64, i64, f64, i64);

pub fn escape_sql_like(s: &str) -> String {
    s.replace('\\', "\\\\")
        .replace('%', "\\%")
        .replace('_', "\\_")
}

pub fn generate_slug(title: &Option<String>) -> String {
    title
        .as_ref()
        .map(|t| {
            t.to_lowercase()
                .chars()
                .filter(|c| c.is_alphanumeric() || *c == ' ')
                .collect::<String>()
                .split_whitespace()
                .collect::<Vec<_>>()
                .join("-")
        })
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "untitled".to_string())
}

pub struct SessionStore {
    pool: SqlitePool,
}

impl SessionStore {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }

    pub fn pool(&self) -> SqlitePool {
        self.pool.clone()
    }

    pub async fn create(&self, input: CreateSession) -> Result<Session, StorageError> {
        let id = Uuid::new_v4().to_string();
        let slug = generate_slug(&input.title);
        let title = input.title.unwrap_or_else(|| "Untitled".to_string());
        let version = "1".to_string();
        let now = Utc::now().timestamp_millis();
        let tags = input.tags.unwrap_or_default();
        let tags_json = serde_json::to_string(&tags)
            .map_err(|e| StorageError::Database(format!("failed to serialize tags: {}", e)))?;

        // Ensure project exists before creating session (foreign key constraint)
        sqlx::query(
            r#"
            INSERT OR IGNORE INTO project (id, worktree, sandboxes, time_created, time_updated)
            VALUES (?, ?, '[]', ?, ?)
            "#,
        )
        .bind(&input.project_id)
        .bind(&input.directory)
        .bind(now)
        .bind(now)
        .execute(&self.pool)
        .await
        .map_err(|e| StorageError::Database(e.to_string()))?;

        sqlx::query(
            r#"
            INSERT INTO session (
                id, project_id, workspace_id, parent_id, slug, directory,
                title, version, tags, time_created, time_updated
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
            "#,
        )
        .bind(&id)
        .bind(&input.project_id)
        .bind(&input.workspace_id)
        .bind(&input.parent_id)
        .bind(&slug)
        .bind(&input.directory)
        .bind(&title)
        .bind(&version)
        .bind(&tags_json)
        .bind(now)
        .bind(now)
        .execute(&self.pool)
        .await
        .map_err(|e| StorageError::Database(e.to_string()))?;

        Ok(Session {
            id,
            project_id: input.project_id,
            workspace_id: input.workspace_id,
            parent_id: input.parent_id,
            slug,
            directory: input.directory,
            title,
            version,
            tags,
            time_created: now,
            time_updated: now,
            time_archived: None,
            share_url: None,
            summary_additions: None,
            summary_deletions: None,
            summary_files: None,
            summary_diffs: None,
            revert: None,
            permission: None,
            time_compacting: None,
            time_deleted: None,
        })
    }

    pub async fn create_from_template(
        &self,
        template: &SessionTemplate,
        project_id: &str,
        directory: &str,
    ) -> Result<Session, StorageError> {
        let input = CreateSession {
            project_id: project_id.to_string(),
            directory: directory.to_string(),
            title: Some(template.name.clone()),
            parent_id: None,
            workspace_id: None,
            agent: template.agent.clone(),
            model: template.model.clone(),
            tags: template.tags.clone(),
        };
        self.create(input).await
    }

    pub async fn get(&self, id: &str) -> Result<Option<Session>, StorageError> {
        sqlx::query_as::<_, SessionRow>(&format!(
            "SELECT {} FROM session WHERE id = ? AND time_deleted IS NULL",
            SESSION_COLUMNS
        ))
        .bind(id)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| StorageError::Database(e.to_string()))
        .map(|row| row.map(|r| r.into()))
    }

    pub async fn list(&self, project_id: &str, limit: usize) -> Result<Vec<Session>, StorageError> {
        self.list_with_offset(project_id, limit, 0).await
    }

    pub async fn list_with_offset(
        &self,
        project_id: &str,
        limit: usize,
        offset: usize,
    ) -> Result<Vec<Session>, StorageError> {
        sqlx::query_as::<_, SessionRow>(&format!(
            "SELECT {} FROM session WHERE project_id = ? AND time_archived IS NULL AND time_deleted IS NULL ORDER BY time_updated DESC LIMIT ? OFFSET ?",
            SESSION_COLUMNS
        ))
        .bind(project_id)
        .bind(limit as i64)
        .bind(offset as i64)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| StorageError::Database(e.to_string()))
        .map(|rows| rows.into_iter().map(|r| r.into()).collect())
    }

    pub async fn session_count(&self, project_id: &str) -> Result<usize, StorageError> {
        let count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM session WHERE project_id = ? AND time_archived IS NULL AND time_deleted IS NULL",
        )
        .bind(project_id)
        .fetch_one(&self.pool)
        .await
        .map_err(|e| StorageError::Database(e.to_string()))?;
        Ok(count as usize)
    }

    pub async fn message_count(&self, session_id: &str) -> Result<usize, StorageError> {
        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM message WHERE session_id = ?")
            .bind(session_id)
            .fetch_one(&self.pool)
            .await
            .map_err(|e| StorageError::Database(e.to_string()))?;
        Ok(count as usize)
    }

    pub async fn message_counts(
        &self,
        session_ids: &[String],
    ) -> Result<HashMap<String, usize>, StorageError> {
        if session_ids.is_empty() {
            return Ok(HashMap::new());
        }
        let placeholders: Vec<String> = session_ids.iter().map(|_| "?".to_string()).collect();
        let query = format!(
            "SELECT session_id, COUNT(*) as count FROM message WHERE session_id IN ({}) GROUP BY session_id",
            placeholders.join(", ")
        );
        let mut query_builder = sqlx::query_as::<_, (String, i64)>(&query);
        for id in session_ids {
            query_builder = query_builder.bind(id);
        }
        let rows: Vec<(String, i64)> = query_builder
            .fetch_all(&self.pool)
            .await
            .map_err(|e| StorageError::Database(e.to_string()))?;
        let mut result = HashMap::new();
        for (session_id, count) in rows {
            result.insert(session_id, count as usize);
        }
        Ok(result)
    }

    pub async fn list_all(
        &self,
        project_id: &str,
        limit: Option<usize>,
    ) -> Result<Vec<Session>, StorageError> {
        self.list_all_with_offset(project_id, limit, 0).await
    }

    pub async fn list_all_with_offset(
        &self,
        project_id: &str,
        limit: Option<usize>,
        offset: usize,
    ) -> Result<Vec<Session>, StorageError> {
        if let Some(limit) = limit {
            let sql = format!(
                "SELECT {} FROM session WHERE project_id = ? AND time_deleted IS NULL ORDER BY time_updated DESC LIMIT ? OFFSET ?",
                SESSION_COLUMNS
            );
            sqlx::query_as::<_, SessionRow>(&sql)
                .bind(project_id)
                .bind(limit as i64)
                .bind(offset as i64)
                .fetch_all(&self.pool)
                .await
                .map_err(|e| StorageError::Database(e.to_string()))
                .map(|rows| rows.into_iter().map(|r| r.into()).collect())
        } else {
            let sql = format!(
                "SELECT {} FROM session WHERE project_id = ? AND time_deleted IS NULL ORDER BY time_updated DESC",
                SESSION_COLUMNS
            );
            sqlx::query_as::<_, SessionRow>(&sql)
                .bind(project_id)
                .fetch_all(&self.pool)
                .await
                .map_err(|e| StorageError::Database(e.to_string()))
                .map(|rows| rows.into_iter().map(|r| r.into()).collect())
        }
    }

    pub async fn search(
        &self,
        project_id: &str,
        query: &str,
    ) -> Result<Vec<Session>, StorageError> {
        let escaped = escape_sql_like(query);
        let pattern = format!("%{}%", escaped);
        sqlx::query_as::<_, SessionRow>(&format!(
            "SELECT {} FROM session WHERE project_id = ? AND (title LIKE ? OR slug LIKE ? OR directory LIKE ?) AND time_archived IS NULL AND time_deleted IS NULL ORDER BY time_updated DESC",
            SESSION_COLUMNS
        ))
        .bind(project_id)
        .bind(&pattern)
        .bind(&pattern)
        .bind(&pattern)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| StorageError::Database(e.to_string()))
        .map(|rows| rows.into_iter().map(|r| r.into()).collect())
    }

    pub async fn search_all(
        &self,
        project_id: &str,
        query: &str,
    ) -> Result<Vec<Session>, StorageError> {
        let escaped = escape_sql_like(query);
        let pattern = format!("%{}%", escaped);
        sqlx::query_as::<_, SessionRow>(&format!(
            "SELECT DISTINCT {} FROM session s LEFT JOIN message m ON s.id = m.session_id WHERE s.project_id = ? AND (s.title LIKE ? OR s.slug LIKE ? OR s.directory LIKE ? OR m.data LIKE ?) AND s.time_archived IS NULL AND s.time_deleted IS NULL ORDER BY s.time_updated DESC",
            SESSION_COLUMNS_QUALIFIED
        ))
        .bind(project_id)
        .bind(&pattern)
        .bind(&pattern)
        .bind(&pattern)
        .bind(&pattern)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| StorageError::Database(e.to_string()))
        .map(|rows| rows.into_iter().map(|r| r.into()).collect())
    }

    pub async fn find_by_tag(
        &self,
        project_id: &str,
        tag: &str,
    ) -> Result<Vec<Session>, StorageError> {
        let escaped = escape_sql_like(tag);
        let pattern = format!("%\"{}%\"", escaped);
        sqlx::query_as::<_, SessionRow>(&format!(
            "SELECT {} FROM session WHERE project_id = ? AND tags LIKE ? AND time_archived IS NULL AND time_deleted IS NULL ORDER BY time_updated DESC",
            SESSION_COLUMNS
        ))
        .bind(project_id)
        .bind(&pattern)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| StorageError::Database(e.to_string()))
        .map(|rows| rows.into_iter().map(|r| r.into()).collect())
    }

    pub async fn all_tags(&self, project_id: &str) -> Result<Vec<String>, StorageError> {
        let sessions = sqlx::query_as::<_, SessionRow>(
            "SELECT id, tags FROM session WHERE project_id = ? AND tags IS NOT NULL AND time_archived IS NULL AND time_deleted IS NULL",
        )
        .bind(project_id)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| StorageError::Database(e.to_string()))?;
        let mut tag_counts: HashMap<String, usize> = HashMap::new();
        for row in sessions {
            if let Some(tags_json) = row.tags {
                if let Ok(tags) = serde_json::from_str::<Vec<String>>(&tags_json) {
                    for t in tags {
                        *tag_counts.entry(t).or_insert(0) += 1;
                    }
                }
            }
        }
        let mut tags: Vec<_> = tag_counts.into_iter().collect();
        tags.sort_by_key(|b| std::cmp::Reverse(b.1));
        Ok(tags.into_iter().map(|(t, _)| t).collect())
    }

    pub async fn export_session(
        &self,
        session_id: &str,
    ) -> Result<serde_json::Value, StorageError> {
        let session = self
            .get(session_id)
            .await?
            .ok_or_else(|| StorageError::NotFound(format!("session {session_id}")))?;

        let messages = sqlx::query_as::<_, MessageRow>(
            r#"
            SELECT id, session_id, time_created, time_updated, data
            FROM message WHERE session_id = ?
            ORDER BY time_created ASC, id ASC
            "#,
        )
        .bind(session_id)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| StorageError::Database(e.to_string()))?;

        let parts = sqlx::query_as::<_, PartRow>(
            r#"
            SELECT id, message_id, session_id, time_created, time_updated, data
            FROM part WHERE session_id = ?
            ORDER BY time_created ASC, id ASC
            "#,
        )
        .bind(session_id)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| StorageError::Database(e.to_string()))?;

        let todos = sqlx::query_as::<_, TodoRow>(
            r#"
            SELECT session_id, content, status, priority, position,
                   time_created, time_updated
            FROM todo WHERE session_id = ?
            ORDER BY position ASC
            "#,
        )
        .bind(session_id)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| StorageError::Database(e.to_string()))?;

        Ok(serde_json::json!({
            "session": session,
            "messages": messages.iter().map(|m| {
                serde_json::json!({
                    "id": m.id,
                    "session_id": m.session_id,
                    "time_created": m.time_created,
                    "time_updated": m.time_updated,
                    "data": super::redact_for_export(parse_json_field(&m.data)),
                })
            }).collect::<Vec<_>>(),
            "parts": parts.iter().map(|p| {
                serde_json::json!({
                    "id": p.id,
                    "message_id": p.message_id,
                    "session_id": p.session_id,
                    "time_created": p.time_created,
                    "time_updated": p.time_updated,
                    "data": super::redact_for_export(parse_json_field(&p.data)),
                })
            }).collect::<Vec<_>>(),
            "todos": todos.iter().map(|t| {
                serde_json::json!({
                    "session_id": t.session_id,
                    "content": t.content,
                    "status": t.status,
                    "priority": t.priority,
                    "position": t.position,
                    "time_created": t.time_created,
                    "time_updated": t.time_updated,
                })
            }).collect::<Vec<_>>(),
        }))
    }

    pub async fn import_session(
        &self,
        data: serde_json::Value,
        new_project_id: Option<&str>,
    ) -> Result<Session, StorageError> {
        validate_import_size(&data)?;

        let import: SessionImport = serde::Deserialize::deserialize(data)
            .map_err(|e| StorageError::Import(format!("invalid import format: {}", e)))?;

        let id = Uuid::new_v4().to_string();
        let project_id = new_project_id
            .map(|s| s.to_string())
            .unwrap_or_else(|| import.session.project_id.unwrap_or_default());
        let title = import
            .session
            .title
            .unwrap_or_else(|| "Imported".to_string());
        let slug = generate_slug(&Some(title.clone()));
        let directory = import.session.directory.unwrap_or_default();
        let version = import.session.version.unwrap_or_else(|| "1".to_string());
        let now = chrono::Utc::now().timestamp_millis();

        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(|e| StorageError::Database(e.to_string()))?;

        sqlx::query(
            r#"
            INSERT INTO session (
                id, project_id, workspace_id, parent_id, slug, directory,
                title, version, time_created, time_updated
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
            "#,
        )
        .bind(&id)
        .bind(&project_id)
        .bind(&import.session.workspace_id)
        .bind::<Option<String>>(None)
        .bind(&slug)
        .bind(&directory)
        .bind(&title)
        .bind(&version)
        .bind(now)
        .bind(now)
        .execute(&mut *tx)
        .await
        .map_err(|e| StorageError::Database(e.to_string()))?;

        let mut id_map: HashMap<String, String> = HashMap::new();

        let msg_values: Vec<(String, String, i64, i64, String)> = import
            .messages
            .iter()
            .map(|msg| {
                let new_msg_id = Uuid::new_v4().to_string();
                if let Some(ref old_id) = msg.id {
                    id_map.insert(old_id.clone(), new_msg_id.clone());
                }
                let msg_data = serde_json::to_string(&msg.data)
                    .map_err(|e| StorageError::Database(e.to_string()))?;
                Ok::<_, StorageError>((
                    new_msg_id,
                    id.clone(),
                    msg.time_created.unwrap_or(now),
                    msg.time_updated.unwrap_or(now),
                    msg_data,
                ))
            })
            .collect::<Result<Vec<_>, _>>()?;

        if !msg_values.is_empty() {
            let mut msg_query: QueryBuilder<_> = QueryBuilder::new(
                "INSERT INTO message (id, session_id, time_created, time_updated, data) ",
            );
            msg_query.push_values(&msg_values, |mut b, val| {
                b.push_bind(&val.0)
                    .push_bind(&val.1)
                    .push_bind(val.2)
                    .push_bind(val.3)
                    .push_bind(&val.4);
            });
            msg_query
                .build()
                .execute(&mut *tx)
                .await
                .map_err(|e| StorageError::Database(e.to_string()))?;
        }

        let part_values: Vec<(String, String, String, i64, i64, String)> = import
            .parts
            .iter()
            .map(|part| {
                let new_part_id = Uuid::new_v4().to_string();
                let old_msg_id = part.message_id.as_deref().unwrap_or_default();
                let new_msg_id = id_map
                    .get(old_msg_id)
                    .map(|s| s.as_str())
                    .unwrap_or(old_msg_id);
                let part_data = serde_json::to_string(&part.data)
                    .map_err(|e| StorageError::Database(e.to_string()))?;
                Ok::<_, StorageError>((
                    new_part_id,
                    new_msg_id.to_string(),
                    id.clone(),
                    part.time_created.unwrap_or(now),
                    part.time_updated.unwrap_or(now),
                    part_data,
                ))
            })
            .collect::<Result<Vec<_>, _>>()?;

        if !part_values.is_empty() {
            let mut part_query: QueryBuilder<_> = QueryBuilder::new(
                "INSERT INTO part (id, message_id, session_id, time_created, time_updated, data) ",
            );
            part_query.push_values(&part_values, |mut b, val| {
                b.push_bind(&val.0)
                    .push_bind(&val.1)
                    .push_bind(&val.2)
                    .push_bind(val.3)
                    .push_bind(val.4)
                    .push_bind(&val.5);
            });
            part_query
                .build()
                .execute(&mut *tx)
                .await
                .map_err(|e| StorageError::Database(e.to_string()))?;
        }

        let todo_values: Vec<(String, String, String, String, i64, i64, i64)> = import
            .todos
            .iter()
            .enumerate()
            .map(|(i, todo)| {
                (
                    id.clone(),
                    todo.content.clone(),
                    todo.status.clone(),
                    todo.priority.clone(),
                    i as i64,
                    todo.time_created.unwrap_or(now),
                    todo.time_updated.unwrap_or(now),
                )
            })
            .collect();

        if !todo_values.is_empty() {
            let mut todo_query: QueryBuilder<_> = QueryBuilder::new(
                "INSERT INTO todo (session_id, content, status, priority, position, time_created, time_updated) ",
            );
            todo_query.push_values(&todo_values, |mut b, val| {
                b.push_bind(&val.0)
                    .push_bind(&val.1)
                    .push_bind(&val.2)
                    .push_bind(&val.3)
                    .push_bind(val.4)
                    .push_bind(val.5)
                    .push_bind(val.6);
            });
            todo_query
                .build()
                .execute(&mut *tx)
                .await
                .map_err(|e| StorageError::Database(e.to_string()))?;
        }

        tx.commit()
            .await
            .map_err(|e| StorageError::Database(e.to_string()))?;

        Ok(Session {
            id,
            project_id,
            workspace_id: import.session.workspace_id,
            parent_id: None,
            slug,
            directory,
            title,
            version,
            share_url: None,
            summary_additions: None,
            summary_deletions: None,
            summary_files: None,
            summary_diffs: None,
            revert: None,
            permission: None,
            tags: Vec::new(),
            time_created: now,
            time_updated: now,
            time_compacting: None,
            time_archived: None,
            time_deleted: None,
        })
    }

    pub async fn update(&self, id: &str, input: UpdateSession) -> Result<Session, StorageError> {
        let now = Utc::now().timestamp_millis();

        let result = sqlx::query_as::<_, SessionRow>(
            r#"
            UPDATE session SET
                time_updated = ?,
                title = COALESCE(?, title),
                share_url = COALESCE(?, share_url),
                summary_additions = COALESCE(?, summary_additions),
                summary_deletions = COALESCE(?, summary_deletions),
                summary_files = COALESCE(?, summary_files),
                summary_diffs = COALESCE(?, summary_diffs),
                revert = COALESCE(?, revert),
                permission = COALESCE(?, permission),
                tags = COALESCE(?, tags),
                time_compacting = COALESCE(?, time_compacting),
                time_archived = COALESCE(?, time_archived)
            WHERE id = ?
            RETURNING *
            "#,
        )
        .bind(now)
        .bind(&input.title)
        .bind(&input.share_url)
        .bind(input.summary_additions)
        .bind(input.summary_deletions)
        .bind(input.summary_files)
        .bind(input.summary_diffs.as_ref().map(|v| v.to_string()))
        .bind(input.revert.as_ref().map(|v| v.to_string()))
        .bind(input.permission.as_ref().map(|v| v.to_string()))
        .bind(
            input
                .tags
                .as_ref()
                .and_then(|v| serde_json::to_string(v).ok()),
        )
        .bind(input.time_compacting)
        .bind(input.time_archived)
        .bind(id)
        .fetch_one(&self.pool)
        .await
        .map_err(|e| StorageError::Database(e.to_string()))?;

        Ok(result.into())
    }

    pub async fn delete(&self, id: &str) -> Result<(), StorageError> {
        self.soft_delete(id).await?;
        Ok(())
    }

    pub async fn soft_delete(&self, id: &str) -> Result<Session, StorageError> {
        let now = Utc::now().timestamp_millis();

        let result = sqlx::query_as::<_, SessionRow>(
            "UPDATE session SET time_deleted = ?, time_updated = ? WHERE id = ? RETURNING *",
        )
        .bind(now)
        .bind(now)
        .bind(id)
        .fetch_one(&self.pool)
        .await
        .map_err(|e| StorageError::Database(e.to_string()))?;

        Ok(result.into())
    }

    pub async fn restore(&self, id: &str) -> Result<Session, StorageError> {
        let now = Utc::now().timestamp_millis();

        let result = sqlx::query_as::<_, SessionRow>(
            "UPDATE session SET time_deleted = NULL, time_updated = ? WHERE id = ? RETURNING *",
        )
        .bind(now)
        .bind(id)
        .fetch_one(&self.pool)
        .await
        .map_err(|e| StorageError::Database(e.to_string()))?;

        Ok(result.into())
    }

    pub async fn list_deleted(&self, project_id: &str) -> Result<Vec<Session>, StorageError> {
        sqlx::query_as::<_, SessionRow>(&format!(
            "SELECT {} FROM session WHERE project_id = ? AND time_deleted IS NOT NULL ORDER BY time_deleted DESC",
            SESSION_COLUMNS
        ))
        .bind(project_id)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| StorageError::Database(e.to_string()))
        .map(|rows| rows.into_iter().map(|r| r.into()).collect())
    }

    pub async fn set_tags(&self, id: &str, tags: Vec<String>) -> Result<Session, StorageError> {
        let tags_json = serde_json::to_string(&tags)
            .map_err(|e| StorageError::Database(format!("failed to serialize tags: {}", e)))?;
        let now = Utc::now().timestamp_millis();

        let result = sqlx::query_as::<_, SessionRow>(
            "UPDATE session SET time_updated = ?, tags = ? WHERE id = ? RETURNING *",
        )
        .bind(now)
        .bind(&tags_json)
        .bind(id)
        .fetch_one(&self.pool)
        .await
        .map_err(|e| StorageError::Database(e.to_string()))?;

        Ok(result.into())
    }

    pub async fn fork(&self, id: &str) -> Result<Session, StorageError> {
        let parent = self
            .get(id)
            .await?
            .ok_or_else(|| StorageError::NotFound(format!("session {id}")))?;

        let child_id = Uuid::new_v4().to_string();
        let slug = generate_slug(&Some(format!("{} (fork)", parent.title)));
        let title = format!("{} (fork)", parent.title);
        let version = "1".to_string();
        let now = Utc::now().timestamp_millis();

        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(|e| StorageError::Database(e.to_string()))?;

        sqlx::query(
            r#"
            INSERT INTO session (
                id, project_id, workspace_id, parent_id, slug, directory,
                title, version, time_created, time_updated, tags
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
            "#,
        )
        .bind(&child_id)
        .bind(&parent.project_id)
        .bind(&parent.workspace_id)
        .bind(id)
        .bind(&slug)
        .bind(&parent.directory)
        .bind(&title)
        .bind(&version)
        .bind(now)
        .bind(now)
        .bind(serde_json::to_string(&parent.tags).unwrap_or_else(|_| "[]".to_string()))
        .execute(&mut *tx)
        .await
        .map_err(|e| StorageError::Database(e.to_string()))?;

        let msgs = sqlx::query_as::<_, MessageRow>(
            r#"
            SELECT id, session_id, time_created, time_updated, data
            FROM message WHERE session_id = ?
            ORDER BY time_created ASC, id ASC
            "#,
        )
        .bind(id)
        .fetch_all(&mut *tx)
        .await
        .map_err(|e| StorageError::Database(e.to_string()))?;

        let mut id_map: HashMap<String, String> = HashMap::new();
        let mut msg_values: Vec<(String, String, i64, i64, String)> = Vec::new();

        for msg in &msgs {
            let new_msg_id = Uuid::new_v4().to_string();
            id_map.insert(msg.id.clone(), new_msg_id.clone());
            let redacted_data =
                serde_json::to_string(&super::redact_for_export(parse_json_field(&msg.data)))
                    .unwrap_or_else(|_| msg.data.clone());
            msg_values.push((
                new_msg_id,
                child_id.clone(),
                msg.time_created,
                msg.time_updated,
                redacted_data,
            ));
        }

        if !msg_values.is_empty() {
            let mut msg_query: QueryBuilder<_> = QueryBuilder::new(
                "INSERT INTO message (id, session_id, time_created, time_updated, data) ",
            );
            msg_query.push_values(&msg_values, |mut b, val| {
                b.push_bind(&val.0)
                    .push_bind(&val.1)
                    .push_bind(val.2)
                    .push_bind(val.3)
                    .push_bind(&val.4);
            });
            msg_query
                .build()
                .execute(&mut *tx)
                .await
                .map_err(|e| StorageError::Database(e.to_string()))?;
        }

        let parts = sqlx::query_as::<_, PartRow>(
            r#"
            SELECT id, message_id, session_id, time_created, time_updated, data
            FROM part WHERE session_id = ?
            ORDER BY time_created ASC, id ASC
            "#,
        )
        .bind(id)
        .fetch_all(&mut *tx)
        .await
        .map_err(|e| StorageError::Database(e.to_string()))?;

        let mut part_values: Vec<(String, String, String, i64, i64, String)> = Vec::new();
        for part in &parts {
            let new_part_id = Uuid::new_v4().to_string();
            let new_msg_id = id_map.get(&part.message_id).unwrap_or(&part.message_id);
            let redacted_data =
                serde_json::to_string(&super::redact_for_export(parse_json_field(&part.data)))
                    .unwrap_or_else(|_| part.data.clone());
            part_values.push((
                new_part_id,
                new_msg_id.clone(),
                child_id.clone(),
                part.time_created,
                part.time_updated,
                redacted_data,
            ));
        }

        if !part_values.is_empty() {
            let mut part_query: QueryBuilder<_> = QueryBuilder::new(
                "INSERT INTO part (id, message_id, session_id, time_created, time_updated, data) ",
            );
            part_query.push_values(&part_values, |mut b, val| {
                b.push_bind(&val.0)
                    .push_bind(&val.1)
                    .push_bind(&val.2)
                    .push_bind(val.3)
                    .push_bind(val.4)
                    .push_bind(&val.5);
            });
            part_query
                .build()
                .execute(&mut *tx)
                .await
                .map_err(|e| StorageError::Database(e.to_string()))?;
        }

        let todos = sqlx::query_as::<_, TodoRow>(
            r#"
            SELECT session_id, content, status, priority, position,
                   time_created, time_updated
            FROM todo WHERE session_id = ?
            ORDER BY position ASC
            "#,
        )
        .bind(id)
        .fetch_all(&mut *tx)
        .await
        .map_err(|e| StorageError::Database(e.to_string()))?;

        let mut todo_values: Vec<(String, String, String, String, i64, i64, i64)> = Vec::new();
        for todo in &todos {
            todo_values.push((
                child_id.clone(),
                todo.content.clone(),
                todo.status.clone(),
                todo.priority.clone(),
                todo.position,
                todo.time_created,
                todo.time_updated,
            ));
        }

        if !todo_values.is_empty() {
            let mut todo_query: QueryBuilder<_> = QueryBuilder::new(
                "INSERT INTO todo (session_id, content, status, priority, position, time_created, time_updated) ",
            );
            todo_query.push_values(&todo_values, |mut b, val| {
                b.push_bind(&val.0)
                    .push_bind(&val.1)
                    .push_bind(&val.2)
                    .push_bind(&val.3)
                    .push_bind(val.4)
                    .push_bind(val.5)
                    .push_bind(val.6);
            });
            todo_query
                .build()
                .execute(&mut *tx)
                .await
                .map_err(|e| StorageError::Database(e.to_string()))?;
        }

        tx.commit()
            .await
            .map_err(|e| StorageError::Database(e.to_string()))?;

        Ok(Session {
            id: child_id.clone(),
            project_id: parent.project_id,
            workspace_id: parent.workspace_id,
            parent_id: Some(id.to_string()),
            slug,
            directory: parent.directory,
            title,
            version,
            share_url: None,
            summary_additions: None,
            summary_deletions: None,
            summary_files: None,
            summary_diffs: None,
            revert: None,
            permission: None,
            tags: parent.tags,
            time_created: now,
            time_updated: now,
            time_compacting: None,
            time_archived: None,
            time_deleted: None,
        })
    }

    pub async fn archive(&self, id: &str) -> Result<Session, StorageError> {
        let now = Utc::now().timestamp_millis();

        let result = sqlx::query_as::<_, SessionRow>(
            "UPDATE session SET time_archived = ?, time_updated = ? WHERE id = ? RETURNING *",
        )
        .bind(now)
        .bind(now)
        .bind(id)
        .fetch_one(&self.pool)
        .await
        .map_err(|e| StorageError::Database(e.to_string()))?;

        Ok(result.into())
    }

    pub async fn unarchive(&self, id: &str) -> Result<Session, StorageError> {
        let now = Utc::now().timestamp_millis();

        let result = sqlx::query_as::<_, SessionRow>(
            "UPDATE session SET time_archived = NULL, time_updated = ? WHERE id = ? RETURNING *",
        )
        .bind(now)
        .bind(id)
        .fetch_one(&self.pool)
        .await
        .map_err(|e| StorageError::Database(e.to_string()))?;

        Ok(result.into())
    }

    pub async fn set_share_url(&self, id: &str, url: &str) -> Result<Session, StorageError> {
        let now = Utc::now().timestamp_millis();

        let result = sqlx::query_as::<_, SessionRow>(
            "UPDATE session SET share_url = ?, time_updated = ? WHERE id = ? RETURNING *",
        )
        .bind(url)
        .bind(now)
        .bind(id)
        .fetch_one(&self.pool)
        .await
        .map_err(|e| StorageError::Database(e.to_string()))?;

        Ok(result.into())
    }

    pub async fn children(&self, id: &str) -> Result<Vec<Session>, StorageError> {
        sqlx::query_as::<_, SessionRow>(&format!(
            "SELECT {} FROM session WHERE parent_id = ? AND time_deleted IS NULL ORDER BY time_created ASC",
            SESSION_COLUMNS
        ))
        .bind(id)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| StorageError::Database(e.to_string()))
        .map(|rows| rows.into_iter().map(|r| r.into()).collect())
    }

    pub async fn revert_to_message(
        &self,
        session_id: &str,
        message_id: &str,
    ) -> Result<Session, StorageError> {
        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(|e| StorageError::Database(e.to_string()))?;

        let msgs = sqlx::query_as::<_, MessageRow>(
            r#"
            SELECT id, session_id, time_created, time_updated, data
            FROM message WHERE session_id = ?
            ORDER BY time_created ASC, id ASC
            "#,
        )
        .bind(session_id)
        .fetch_all(&mut *tx)
        .await
        .map_err(|e| StorageError::Database(e.to_string()))?;

        let pivot = msgs
            .iter()
            .position(|m| m.id == message_id)
            .ok_or_else(|| {
                StorageError::NotFound(format!("message {message_id} in session {session_id}"))
            })?;

        let to_delete: Vec<&MessageRow> = msgs.iter().skip(pivot + 1).collect();
        let removed = to_delete.len();
        let msg_ids: Vec<String> = to_delete.iter().map(|m| m.id.clone()).collect();

        let mut saved_messages = Vec::new();

        if !msg_ids.is_empty() {
            let placeholders: Vec<String> = msg_ids.iter().map(|_| "?".to_string()).collect();
            let in_clause = placeholders.join(",");
            let all_parts_query = format!(
                "SELECT id, message_id, session_id, time_created, time_updated, data FROM part WHERE message_id IN ({}) ORDER BY time_created ASC, id ASC",
                in_clause
            );
            let mut query_builder = sqlx::query_as::<_, PartRow>(&all_parts_query);
            for id in &msg_ids {
                query_builder = query_builder.bind(id);
            }
            let all_parts = query_builder
                .fetch_all(&mut *tx)
                .await
                .map_err(|e| StorageError::Database(e.to_string()))?;

            let mut parts_by_msg: std::collections::HashMap<String, Vec<PartRow>> =
                std::collections::HashMap::new();
            for part in all_parts {
                parts_by_msg
                    .entry(part.message_id.clone())
                    .or_default()
                    .push(part);
            }

            for msg_row in &to_delete {
                let parts = parts_by_msg.get(&msg_row.id);
                let saved_parts: Vec<serde_json::Value> = parts
                    .map(|parts| {
                        parts
                            .iter()
                            .map(|p| {
                                serde_json::json!({
                                    "id": p.id,
                                    "message_id": p.message_id,
                                    "time_created": p.time_created,
                                    "time_updated": p.time_updated,
                                    "data": parse_json_field(&p.data),
                                })
                            })
                            .collect()
                    })
                    .unwrap_or_default();

                saved_messages.push(serde_json::json!({
                    "id": msg_row.id,
                    "time_created": msg_row.time_created,
                    "time_updated": msg_row.time_updated,
                    "data": parse_json_field(&msg_row.data),
                    "parts": saved_parts,
                }));
            }

            let delete_parts_query =
                format!("DELETE FROM part WHERE message_id IN ({})", in_clause);
            let mut query_builder = sqlx::query(&delete_parts_query);
            for id in &msg_ids {
                query_builder = query_builder.bind(id);
            }
            query_builder
                .execute(&mut *tx)
                .await
                .map_err(|e| StorageError::Database(e.to_string()))?;
        }

        if !to_delete.is_empty() {
            let delete_msg_query = format!(
                "DELETE FROM message WHERE id IN ({})",
                to_delete.iter().map(|_| "?").collect::<Vec<_>>().join(",")
            );
            let mut query_builder = sqlx::query(&delete_msg_query);
            for msg_row in &to_delete {
                query_builder = query_builder.bind(&msg_row.id);
            }
            query_builder
                .execute(&mut *tx)
                .await
                .map_err(|e| StorageError::Database(e.to_string()))?;
        }

        let revert_info = serde_json::json!({
            "reverted_at": chrono::Utc::now().timestamp_millis(),
            "to_message": message_id,
            "removed_count": removed,
            "messages": saved_messages,
        });

        let now = Utc::now().timestamp_millis();
        let revert_json = serde_json::to_string(&revert_info).map_err(|e| {
            StorageError::Database(format!("failed to serialize revert info: {}", e))
        })?;
        sqlx::query(
            r#"
            UPDATE session SET
                time_updated = ?,
                revert = ?
            WHERE id = ?
            "#,
        )
        .bind(now)
        .bind(revert_json)
        .bind(session_id)
        .execute(&mut *tx)
        .await
        .map_err(|e| StorageError::Database(e.to_string()))?;

        tx.commit()
            .await
            .map_err(|e| StorageError::Database(e.to_string()))?;

        self.get(session_id)
            .await?
            .ok_or_else(|| StorageError::NotFound(format!("session {session_id}")))
    }

    pub async fn generate_summary(
        &self,
        provider: &impl super::models::SessionSummaryProvider,
        session_id: &str,
    ) -> Result<Session, StorageError> {
        let convo = self.get_conversation_text(session_id).await?;
        let summary =
            provider
                .generate_summary(&convo)
                .await
                .map_err(|e| StorageError::LlmOperation {
                    operation: "generate_summary".to_string(),
                    message: e.to_string(),
                })?;

        let update = UpdateSession {
            title: None,
            share_url: None,
            summary_additions: None,
            summary_deletions: None,
            summary_files: None,
            summary_diffs: Some(serde_json::json!({ "text": summary })),
            revert: None,
            permission: None,
            tags: None,
            time_compacting: None,
            time_archived: None,
        };

        self.update(session_id, update).await
    }

    pub async fn generate_title(
        &self,
        provider: &impl super::models::SessionSummaryProvider,
        session_id: &str,
    ) -> Result<Session, StorageError> {
        let convo = self.get_conversation_text(session_id).await?;
        let title =
            provider
                .generate_title(&convo)
                .await
                .map_err(|e| StorageError::LlmOperation {
                    operation: "generate_title".to_string(),
                    message: e.to_string(),
                })?;

        let update = UpdateSession {
            title: Some(title),
            share_url: None,
            summary_additions: None,
            summary_deletions: None,
            summary_files: None,
            summary_diffs: None,
            revert: None,
            permission: None,
            tags: None,
            time_compacting: None,
            time_archived: None,
        };

        self.update(session_id, update).await
    }

    async fn get_conversation_text(&self, session_id: &str) -> Result<String, StorageError> {
        let msgs = sqlx::query_as::<_, MessageRow>(
            r#"
            SELECT id, session_id, time_created, time_updated, data
            FROM message WHERE session_id = ?
            ORDER BY time_created ASC, id ASC
            "#,
        )
        .bind(session_id)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| StorageError::Database(e.to_string()))?;

        let mut result = String::new();
        for msg in &msgs {
            if let Ok(data) = serde_json::from_str::<message::MessageData>(&msg.data) {
                for part in &data.parts {
                    if let message::PartData::Text { text } = &part.data {
                        if !result.is_empty() {
                            result.push('\n');
                        }
                        result.push_str(text);
                    }
                }
            }
        }

        Ok(result)
    }

    pub async fn share_session(&self, session_id: &str) -> Result<Session, StorageError> {
        const DEFAULT_SHARE_DURATION_DAYS: i64 = 7;
        let share_duration_days = std::env::var("CODEGG_SHARE_DURATION_DAYS")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(DEFAULT_SHARE_DURATION_DAYS);

        let now = Utc::now().timestamp_millis();
        let share_expires_at = now + (share_duration_days * 24 * 60 * 60 * 1000);

        let mut token_bytes = [0u8; 32];
        rand::rng().fill_bytes(&mut token_bytes);
        let token = base64::Engine::encode(
            &base64::engine::general_purpose::URL_SAFE_NO_PAD,
            token_bytes,
        );

        let share_id = uuid::Uuid::new_v4().to_string();
        let url = format!("codegg://share/{token}");

        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(|e| StorageError::Database(e.to_string()))?;

        sqlx::query(
            r#"
            INSERT INTO session_share (session_id, id, secret, url, share_expires_at, time_created, time_updated)
            VALUES (?, ?, ?, ?, ?, ?, ?)
            ON CONFLICT(session_id) DO UPDATE SET
                id = excluded.id,
                secret = excluded.secret,
                url = excluded.url,
                share_expires_at = excluded.share_expires_at,
                time_updated = excluded.time_updated
            "#,
        )
        .bind(session_id)
        .bind(&share_id)
        .bind(&token)
        .bind(&url)
        .bind(share_expires_at)
        .bind(now)
        .bind(now)
        .execute(&mut *tx)
        .await
        .map_err(|e| StorageError::Database(e.to_string()))?;

        let result = sqlx::query_as::<_, SessionRow>(
            "UPDATE session SET share_url = ?, time_updated = ? WHERE id = ? RETURNING *",
        )
        .bind(&url)
        .bind(now)
        .bind(session_id)
        .fetch_one(&mut *tx)
        .await
        .map_err(|e| StorageError::Database(e.to_string()))?;

        tx.commit()
            .await
            .map_err(|e| StorageError::Database(e.to_string()))?;

        Ok(result.into())
    }

    pub async fn unshare_session(&self, session_id: &str) -> Result<Session, StorageError> {
        let now = Utc::now().timestamp_millis();

        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(|e| StorageError::Database(e.to_string()))?;

        let result = sqlx::query_as::<_, SessionRow>(
            "UPDATE session SET share_url = NULL, time_updated = ? WHERE id = ? RETURNING *",
        )
        .bind(now)
        .bind(session_id)
        .fetch_one(&mut *tx)
        .await
        .map_err(|e| StorageError::Database(e.to_string()))?;

        sqlx::query("DELETE FROM session_share WHERE session_id = ?")
            .bind(session_id)
            .execute(&mut *tx)
            .await
            .map_err(|e| StorageError::Database(e.to_string()))?;

        tx.commit()
            .await
            .map_err(|e| StorageError::Database(e.to_string()))?;

        Ok(result.into())
    }

    pub async fn unrevert_session(&self, session_id: &str) -> Result<Session, StorageError> {
        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(|e| StorageError::Database(e.to_string()))?;

        let session_row: Option<SessionRow> = sqlx::query_as::<_, SessionRow>(&format!(
            "SELECT {} FROM session WHERE id = ?",
            SESSION_COLUMNS
        ))
        .bind(session_id)
        .fetch_optional(&mut *tx)
        .await
        .map_err(|e| StorageError::Database(e.to_string()))?;

        let revert_info: Session = session_row
            .map(|r| r.into())
            .ok_or_else(|| StorageError::NotFound(format!("session {session_id}")))?;

        let prev = revert_info
            .revert
            .as_ref()
            .ok_or_else(|| StorageError::Database("no revert state to unrevert".to_string()))?;

        let prev_msgs = prev["messages"].as_array().ok_or_else(|| {
            StorageError::Database("invalid revert state: missing messages".to_string())
        })?;

        let now = Utc::now().timestamp_millis();

        let mut msgs: Vec<(String, String, i64, i64, String)> = Vec::new();
        let mut parts: Vec<(String, String, String, i64, i64, String)> = Vec::new();

        for msg_json in prev_msgs {
            let msg_id = msg_json["id"].as_str().unwrap_or_default().to_string();
            let data = &msg_json["data"];
            let created = msg_json["time_created"].as_i64().unwrap_or(now);
            let updated = msg_json["time_updated"].as_i64().unwrap_or(now);
            let data_str =
                serde_json::to_string(data).map_err(|e| StorageError::Database(e.to_string()))?;

            msgs.push((
                msg_id.clone(),
                session_id.to_string(),
                created,
                updated,
                data_str,
            ));

            if let Some(msg_parts) = msg_json["parts"].as_array() {
                for part_json in msg_parts {
                    let part_id = part_json["id"].as_str().unwrap_or_default().to_string();
                    let part_data = &part_json["data"];
                    let part_msg_id = part_json["message_id"]
                        .as_str()
                        .unwrap_or(&msg_id)
                        .to_string();
                    let p_created = part_json["time_created"].as_i64().unwrap_or(now);
                    let p_updated = part_json["time_updated"].as_i64().unwrap_or(now);
                    let part_data_str = serde_json::to_string(part_data)
                        .map_err(|e| StorageError::Database(e.to_string()))?;

                    parts.push((
                        part_id,
                        part_msg_id,
                        session_id.to_string(),
                        p_created,
                        p_updated,
                        part_data_str,
                    ));
                }
            }
        }

        if !msgs.is_empty() {
            let mut msg_query: QueryBuilder<_> = QueryBuilder::new(
                "INSERT OR IGNORE INTO message (id, session_id, time_created, time_updated, data) ",
            );
            msg_query.push_values(&msgs, |mut b, (id, sess_id, created, updated, data)| {
                b.push_bind(id)
                    .push_bind(sess_id)
                    .push_bind(created)
                    .push_bind(updated)
                    .push_bind(data);
            });
            msg_query
                .build()
                .execute(&mut *tx)
                .await
                .map_err(|e| StorageError::Database(e.to_string()))?;
        }

        if !parts.is_empty() {
            let mut part_query: QueryBuilder<_> = QueryBuilder::new(
                "INSERT OR IGNORE INTO part (id, message_id, session_id, time_created, time_updated, data) ",
            );
            part_query.push_values(
                &parts,
                |mut b, (id, msg_id, sess_id, created, updated, data)| {
                    b.push_bind(id)
                        .push_bind(msg_id)
                        .push_bind(sess_id)
                        .push_bind(created)
                        .push_bind(updated)
                        .push_bind(data);
                },
            );
            part_query
                .build()
                .execute(&mut *tx)
                .await
                .map_err(|e| StorageError::Database(e.to_string()))?;
        }

        sqlx::query("UPDATE session SET time_updated = ?, revert = NULL WHERE id = ?")
            .bind(now)
            .bind(session_id)
            .execute(&mut *tx)
            .await
            .map_err(|e| StorageError::Database(e.to_string()))?;

        tx.commit()
            .await
            .map_err(|e| StorageError::Database(e.to_string()))?;

        let mut session = revert_info;
        session.time_updated = now;
        session.revert = None;
        Ok(session)
    }

    pub async fn get_analytics(&self, project_id: &str) -> Result<SessionAnalytics, StorageError> {
        let total_sessions: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM session WHERE project_id = ? AND time_archived IS NULL AND time_deleted IS NULL",
        )
        .bind(project_id)
        .fetch_one(&self.pool)
        .await
        .map_err(|e| StorageError::Database(e.to_string()))?;

        let total_messages: i64 = sqlx::query_scalar(
            r#"
            SELECT COUNT(*) FROM message m
            JOIN session s ON m.session_id = s.id
            WHERE s.project_id = ? AND s.time_archived IS NULL AND s.time_deleted IS NULL
            "#,
        )
        .bind(project_id)
        .fetch_one(&self.pool)
        .await
        .map_err(|e| StorageError::Database(e.to_string()))?;

        let total_tools: i64 = sqlx::query_scalar(
            r#"
            SELECT COUNT(*) FROM part p
            JOIN message m ON p.message_id = m.id
            JOIN session s ON m.session_id = s.id
            WHERE s.project_id = ? AND s.time_archived IS NULL AND s.time_deleted IS NULL
            AND p.part_type = 'tool_call'
            "#,
        )
        .bind(project_id)
        .fetch_one(&self.pool)
        .await
        .map_err(|e| StorageError::Database(e.to_string()))?;

        let avg_duration_ms: Option<i64> = sqlx::query_scalar(
            r#"
            SELECT AVG(time_updated - time_created) FROM session
            WHERE project_id = ? AND time_archived IS NULL AND time_deleted IS NULL
            "#,
        )
        .bind(project_id)
        .fetch_one(&self.pool)
        .await
        .map_err(|e| StorageError::Database(e.to_string()))?;

        Ok(SessionAnalytics {
            total_sessions: total_sessions as u64,
            total_messages: total_messages as u64,
            total_tool_calls: total_tools as u64,
            avg_session_duration_ms: avg_duration_ms.unwrap_or(0) as u64,
        })
    }
}

pub struct TodoStore {
    pool: SqlitePool,
}

impl TodoStore {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }

    pub async fn list(&self, session_id: &str) -> Result<Vec<TodoItem>, StorageError> {
        sqlx::query_as::<_, TodoRow>(
            r#"
            SELECT session_id, content, status, priority, position,
                   time_created, time_updated
            FROM todo WHERE session_id = ?
            ORDER BY position ASC
            "#,
        )
        .bind(session_id)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| StorageError::Database(e.to_string()))
        .map(|rows| rows.into_iter().map(|r| r.into()).collect())
    }

    pub async fn set(
        &self,
        session_id: &str,
        items: Vec<TodoItemInput>,
    ) -> Result<Vec<TodoItem>, StorageError> {
        let now = Utc::now().timestamp_millis();

        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(|e| StorageError::Database(e.to_string()))?;

        sqlx::query("DELETE FROM todo WHERE session_id = ?")
            .bind(session_id)
            .execute(&mut *tx)
            .await
            .map_err(|e| StorageError::Database(e.to_string()))?;

        if !items.is_empty() {
            let todo_values: Vec<(String, String, String, String, i64, i64, i64)> = items
                .iter()
                .enumerate()
                .map(|(i, item)| {
                    (
                        session_id.to_string(),
                        item.content.clone(),
                        item.status.clone(),
                        item.priority.clone(),
                        i as i64,
                        now,
                        now,
                    )
                })
                .collect();

            let mut todo_query: QueryBuilder<_> = QueryBuilder::new(
                "INSERT INTO todo (session_id, content, status, priority, position, time_created, time_updated) ",
            );
            todo_query.push_values(&todo_values, |mut b, val| {
                b.push_bind(&val.0)
                    .push_bind(&val.1)
                    .push_bind(&val.2)
                    .push_bind(&val.3)
                    .push_bind(val.4)
                    .push_bind(val.5)
                    .push_bind(val.6);
            });
            todo_query
                .build()
                .execute(&mut *tx)
                .await
                .map_err(|e| StorageError::Database(e.to_string()))?;
        }

        tx.commit()
            .await
            .map_err(|e| StorageError::Database(e.to_string()))?;

        Ok(items
            .into_iter()
            .enumerate()
            .map(|(i, item)| TodoItem {
                session_id: session_id.to_string(),
                content: item.content,
                status: item.status,
                priority: item.priority,
                position: i as i64,
                time_created: now,
                time_updated: now,
            })
            .collect())
    }

    pub async fn add(
        &self,
        session_id: &str,
        item: TodoItemInput,
    ) -> Result<TodoItem, StorageError> {
        let now = Utc::now().timestamp_millis();

        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(|e| StorageError::Database(e.to_string()))?;

        let position: i64 = sqlx::query_scalar(
            "SELECT COALESCE(MAX(position) + 1, 0) FROM todo WHERE session_id = ?",
        )
        .bind(session_id)
        .fetch_one(&mut *tx)
        .await
        .map_err(|e| StorageError::Database(e.to_string()))?;

        sqlx::query(
            r#"
            INSERT INTO todo (session_id, content, status, priority, position, time_created, time_updated)
            VALUES (?, ?, ?, ?, ?, ?, ?)
            "#,
        )
        .bind(session_id)
        .bind(&item.content)
        .bind(&item.status)
        .bind(&item.priority)
        .bind(position)
        .bind(now)
        .bind(now)
        .execute(&mut *tx)
        .await
        .map_err(|e| StorageError::Database(e.to_string()))?;

        tx.commit()
            .await
            .map_err(|e| StorageError::Database(e.to_string()))?;

        Ok(TodoItem {
            session_id: session_id.to_string(),
            content: item.content,
            status: item.status,
            priority: item.priority,
            position,
            time_created: now,
            time_updated: now,
        })
    }

    pub async fn update(
        &self,
        session_id: &str,
        position: i64,
        item: TodoItemInput,
    ) -> Result<Vec<TodoItem>, StorageError> {
        let now = Utc::now().timestamp_millis();

        sqlx::query(
            r#"
            UPDATE todo SET content = ?, status = ?, priority = ?, time_updated = ?
            WHERE session_id = ? AND position = ?
            "#,
        )
        .bind(&item.content)
        .bind(&item.status)
        .bind(&item.priority)
        .bind(now)
        .bind(session_id)
        .bind(position)
        .execute(&self.pool)
        .await
        .map_err(|e| StorageError::Database(e.to_string()))?;

        self.list(session_id).await
    }

    pub async fn remove(
        &self,
        session_id: &str,
        position: i64,
    ) -> Result<Vec<TodoItem>, StorageError> {
        sqlx::query("DELETE FROM todo WHERE session_id = ? AND position = ?")
            .bind(session_id)
            .bind(position)
            .execute(&self.pool)
            .await
            .map_err(|e| StorageError::Database(e.to_string()))?;

        self.list(session_id).await
    }

    pub async fn clear(&self, session_id: &str) -> Result<(), StorageError> {
        sqlx::query("DELETE FROM todo WHERE session_id = ?")
            .bind(session_id)
            .execute(&self.pool)
            .await
            .map_err(|e| StorageError::Database(e.to_string()))?;

        Ok(())
    }
}

pub struct MessageStore {
    pool: SqlitePool,
}

impl MessageStore {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }

    pub async fn create(
        &self,
        session_id: &str,
        data: serde_json::Value,
    ) -> Result<message::Message, StorageError> {
        let id = Uuid::new_v4().to_string();
        let now = Utc::now().timestamp_millis();
        let data_str =
            serde_json::to_string(&data).map_err(|e| StorageError::Database(e.to_string()))?;

        sqlx::query(
            r#"
            INSERT INTO message (id, session_id, time_created, time_updated, data)
            VALUES (?, ?, ?, ?, ?)
            "#,
        )
        .bind(&id)
        .bind(session_id)
        .bind(now)
        .bind(now)
        .bind(&data_str)
        .execute(&self.pool)
        .await
        .map_err(|e| StorageError::Database(e.to_string()))?;

        let message_data: message::MessageData =
            serde_json::from_value(data).map_err(|e| StorageError::Database(e.to_string()))?;
        Ok(message::Message {
            id,
            session_id: session_id.to_string(),
            time_created: now,
            time_updated: now,
            data: message_data,
        })
    }

    pub async fn get(
        &self,
        session_id: &str,
        id: &str,
    ) -> Result<Option<message::Message>, StorageError> {
        sqlx::query_as::<_, MessageRow>(
            r#"
            SELECT id, session_id, time_created, time_updated, data
            FROM message WHERE session_id = ? AND id = ?
            "#,
        )
        .bind(session_id)
        .bind(id)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| StorageError::Database(e.to_string()))?
        .map(|r| r.try_into())
        .transpose()
        .map_err(|e: StorageError| e)
    }

    pub async fn list(&self, session_id: &str) -> Result<Vec<message::Message>, StorageError> {
        sqlx::query_as::<_, MessageRow>(MESSAGE_QUERY)
            .bind(session_id)
            .fetch_all(&self.pool)
            .await
            .map_err(|e| StorageError::Database(e.to_string()))?
            .into_iter()
            .map(|r| r.try_into())
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e: StorageError| e)
    }

    pub async fn count(&self, session_id: &str) -> Result<usize, StorageError> {
        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM message WHERE session_id = ?")
            .bind(session_id)
            .fetch_one(&self.pool)
            .await
            .map_err(|e| StorageError::Database(e.to_string()))?;
        Ok(count as usize)
    }

    pub async fn update(
        &self,
        session_id: &str,
        id: &str,
        data: serde_json::Value,
    ) -> Result<message::Message, StorageError> {
        let now = Utc::now().timestamp_millis();
        let data_str =
            serde_json::to_string(&data).map_err(|e| StorageError::Database(e.to_string()))?;

        sqlx::query(
            "UPDATE message SET data = ?, time_updated = ? WHERE session_id = ? AND id = ?",
        )
        .bind(&data_str)
        .bind(now)
        .bind(session_id)
        .bind(id)
        .execute(&self.pool)
        .await
        .map_err(|e| StorageError::Database(e.to_string()))?;

        self.get(session_id, id)
            .await?
            .ok_or_else(|| StorageError::NotFound(format!("message {id}")))
    }

    pub async fn delete(&self, session_id: &str, id: &str) -> Result<(), StorageError> {
        sqlx::query("DELETE FROM message WHERE session_id = ? AND id = ?")
            .bind(session_id)
            .bind(id)
            .execute(&self.pool)
            .await
            .map_err(|e| StorageError::Database(e.to_string()))?;

        Ok(())
    }
}

pub struct PartStore {
    pool: SqlitePool,
}

impl PartStore {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }

    pub async fn create(
        &self,
        message_id: &str,
        session_id: &str,
        data: serde_json::Value,
    ) -> Result<message::Part, StorageError> {
        let id = Uuid::new_v4().to_string();
        let now = Utc::now().timestamp_millis();
        let data_str =
            serde_json::to_string(&data).map_err(|e| StorageError::Database(e.to_string()))?;

        sqlx::query(
            r#"
            INSERT INTO part (id, message_id, session_id, time_created, time_updated, data)
            VALUES (?, ?, ?, ?, ?, ?)
            "#,
        )
        .bind(&id)
        .bind(message_id)
        .bind(session_id)
        .bind(now)
        .bind(now)
        .bind(&data_str)
        .execute(&self.pool)
        .await
        .map_err(|e| StorageError::Database(e.to_string()))?;

        self.get(&id)
            .await?
            .ok_or_else(|| StorageError::NotFound(format!("session {id}")))
    }

    pub async fn get(&self, id: &str) -> Result<Option<message::Part>, StorageError> {
        sqlx::query_as::<_, PartRow>(
            r#"
            SELECT id, message_id, session_id, time_created, time_updated, data
            FROM part WHERE id = ?
            "#,
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| StorageError::Database(e.to_string()))
        .map(|row| row.map(|r| r.into()))
    }

    pub async fn list_by_message(
        &self,
        message_id: &str,
    ) -> Result<Vec<message::Part>, StorageError> {
        sqlx::query_as::<_, PartRow>(
            r#"
            SELECT id, message_id, session_id, time_created, time_updated, data
            FROM part WHERE message_id = ?
            ORDER BY time_created ASC, id ASC
            "#,
        )
        .bind(message_id)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| StorageError::Database(e.to_string()))
        .map(|rows| rows.into_iter().map(|r| r.into()).collect())
    }

    pub async fn list_by_session(
        &self,
        session_id: &str,
    ) -> Result<Vec<message::Part>, StorageError> {
        sqlx::query_as::<_, PartRow>(PART_QUERY)
            .bind(session_id)
            .fetch_all(&self.pool)
            .await
            .map_err(|e| StorageError::Database(e.to_string()))
            .map(|rows| rows.into_iter().map(|r| r.into()).collect())
    }

    pub async fn update(
        &self,
        id: &str,
        data: serde_json::Value,
    ) -> Result<message::Part, StorageError> {
        let now = Utc::now().timestamp_millis();
        let data_str =
            serde_json::to_string(&data).map_err(|e| StorageError::Database(e.to_string()))?;

        sqlx::query("UPDATE part SET data = ?, time_updated = ? WHERE id = ?")
            .bind(&data_str)
            .bind(now)
            .bind(id)
            .execute(&self.pool)
            .await
            .map_err(|e| StorageError::Database(e.to_string()))?;

        self.get(id)
            .await?
            .ok_or_else(|| StorageError::NotFound(format!("part {id}")))
    }

    pub async fn delete(&self, id: &str) -> Result<(), StorageError> {
        sqlx::query("DELETE FROM part WHERE id = ?")
            .bind(id)
            .execute(&self.pool)
            .await
            .map_err(|e| StorageError::Database(e.to_string()))?;

        Ok(())
    }
}

pub struct PermissionStore {
    pool: SqlitePool,
}

impl PermissionStore {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }

    pub async fn get(&self, project_id: &str) -> Result<Option<PermissionEntry>, StorageError> {
        sqlx::query_as::<_, super::row::PermissionRow>(
            r#"
            SELECT project_id, time_created, time_updated, data
            FROM permission WHERE project_id = ?
            "#,
        )
        .bind(project_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| StorageError::Database(e.to_string()))
        .map(|row| row.map(|r| r.into()))
    }

    pub async fn upsert(
        &self,
        project_id: &str,
        data: serde_json::Value,
    ) -> Result<PermissionEntry, StorageError> {
        let now = Utc::now().timestamp_millis();
        let data_str =
            serde_json::to_string(&data).map_err(|e| StorageError::Database(e.to_string()))?;

        sqlx::query(
            r#"
            INSERT INTO permission (project_id, time_created, time_updated, data)
            VALUES (?, ?, ?, ?)
            ON CONFLICT(project_id) DO UPDATE SET
                data = excluded.data,
                time_updated = excluded.time_updated
            "#,
        )
        .bind(project_id)
        .bind(now)
        .bind(now)
        .bind(&data_str)
        .execute(&self.pool)
        .await
        .map_err(|e| StorageError::Database(e.to_string()))?;

        self.get(project_id)
            .await?
            .ok_or_else(|| StorageError::NotFound(format!("permission for project {project_id}")))
    }

    pub async fn delete(&self, project_id: &str) -> Result<(), StorageError> {
        sqlx::query("DELETE FROM permission WHERE project_id = ?")
            .bind(project_id)
            .execute(&self.pool)
            .await
            .map_err(|e| StorageError::Database(e.to_string()))?;

        Ok(())
    }
}

pub struct UsageStore {
    pool: SqlitePool,
}

impl UsageStore {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }

    pub async fn insert(&self, record: UsageRecord) -> Result<(), StorageError> {
        sqlx::query(
            r#"
            INSERT INTO usage (id, session_id, provider, model, input_tokens, output_tokens, cached_tokens, cost_usd, timestamp)
            VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)
            "#,
        )
        .bind(&record.id)
        .bind(&record.session_id)
        .bind(&record.provider)
        .bind(&record.model)
        .bind(record.input_tokens)
        .bind(record.output_tokens)
        .bind(record.cached_tokens)
        .bind(record.cost_usd)
        .bind(record.timestamp)
        .execute(&self.pool)
        .await
        .map_err(|e| StorageError::Database(e.to_string()))?;

        Ok(())
    }

    pub async fn get_session_usage(
        &self,
        session_id: &str,
    ) -> Result<Vec<UsageRecord>, StorageError> {
        let rows: Vec<UsageRow> = sqlx::query_as(
            r#"
            SELECT id, session_id, provider, model, input_tokens, output_tokens, cached_tokens, cost_usd, timestamp
            FROM usage WHERE session_id = ?
            ORDER BY timestamp ASC
            "#,
        )
        .bind(session_id)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| StorageError::Database(e.to_string()))?;

        Ok(rows
            .into_iter()
            .map(
                |(
                    id,
                    session_id,
                    provider,
                    model,
                    input_tokens,
                    output_tokens,
                    cached_tokens,
                    cost_usd,
                    timestamp,
                )| {
                    UsageRecord {
                        id,
                        session_id,
                        provider,
                        model,
                        input_tokens,
                        output_tokens,
                        cached_tokens,
                        cost_usd,
                        timestamp,
                    }
                },
            )
            .collect())
    }

    pub async fn get_all_usage(
        &self,
        limit: Option<usize>,
    ) -> Result<Vec<UsageRecord>, StorageError> {
        let rows: Vec<UsageRow> = if let Some(limit) = limit {
            sqlx::query_as(
                r#"
                SELECT id, session_id, provider, model, input_tokens, output_tokens, cached_tokens, cost_usd, timestamp
                FROM usage
                ORDER BY timestamp DESC
                LIMIT ?
                "#,
            )
            .bind(limit as i64)
            .fetch_all(&self.pool)
            .await
            .map_err(|e| StorageError::Database(e.to_string()))?
        } else {
            sqlx::query_as(
                r#"
                SELECT id, session_id, provider, model, input_tokens, output_tokens, cached_tokens, cost_usd, timestamp
                FROM usage
                ORDER BY timestamp DESC
                "#,
            )
            .fetch_all(&self.pool)
            .await
            .map_err(|e| StorageError::Database(e.to_string()))?
        };

        Ok(rows
            .into_iter()
            .map(
                |(
                    id,
                    session_id,
                    provider,
                    model,
                    input_tokens,
                    output_tokens,
                    cached_tokens,
                    cost_usd,
                    timestamp,
                )| {
                    UsageRecord {
                        id,
                        session_id,
                        provider,
                        model,
                        input_tokens,
                        output_tokens,
                        cached_tokens,
                        cost_usd,
                        timestamp,
                    }
                },
            )
            .collect())
    }

    pub async fn get_session_cost_summary(
        &self,
        session_id: &str,
    ) -> Result<(i64, i64, i64, f64), StorageError> {
        let row: Option<(i64, i64, i64, f64)> = sqlx::query_as(
            r#"
            SELECT COALESCE(SUM(input_tokens), 0), COALESCE(SUM(output_tokens), 0), COALESCE(SUM(cached_tokens), 0), COALESCE(SUM(cost_usd), 0.0)
            FROM usage WHERE session_id = ?
            "#,
        )
        .bind(session_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| StorageError::Database(e.to_string()))?;

        Ok(row.unwrap_or((0, 0, 0, 0.0)))
    }
}

pub struct EventStore {
    pool: SqlitePool,
}

impl EventStore {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }

    pub async fn append(&self, event: &super::events::SessionEvent) -> Result<(), StorageError> {
        let meta = event.meta();
        let event_type = event.event_type_tag();
        let payload_json =
            serde_json::to_string(event).map_err(|e| StorageError::Database(e.to_string()))?;
        let created_at = meta.created_at.to_rfc3339();

        sqlx::query(
            r#"
            INSERT INTO session_events (id, session_id, created_at, event_type, payload_json)
            VALUES (?, ?, ?, ?, ?)
            "#,
        )
        .bind(&meta.id)
        .bind(&meta.session_id)
        .bind(&created_at)
        .bind(event_type)
        .bind(&payload_json)
        .execute(&self.pool)
        .await
        .map_err(|e| StorageError::Database(e.to_string()))?;

        Ok(())
    }

    pub async fn list_for_session(
        &self,
        session_id: &str,
    ) -> Result<Vec<super::events::SessionEvent>, StorageError> {
        let rows: Vec<(String,)> = sqlx::query_as(
            r#"
            SELECT payload_json FROM session_events
            WHERE session_id = ?
            ORDER BY created_at ASC, id ASC
            "#,
        )
        .bind(session_id)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| StorageError::Database(e.to_string()))?;

        rows.into_iter()
            .map(|(payload,)| {
                serde_json::from_str::<super::events::SessionEvent>(&payload).map_err(|e| {
                    StorageError::Database(format!("failed to deserialize event: {}", e))
                })
            })
            .collect()
    }
}

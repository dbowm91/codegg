use std::path::{Component, Path, PathBuf};

use async_trait::async_trait;
use serde_json::json;

use crate::error::ToolError;
use crate::tool::{StructuredToolResult, Tool, ToolCategory, ToolExecutionContext};
use codegg_core::session::SessionWorkingDirectoryStore;

pub struct SessionWorkingDirectoryTool {
    store: SessionWorkingDirectoryStore,
    session_id: String,
    workspace_root: PathBuf,
}

const MAX_SESSION_WORKING_DIRECTORY_BYTES: usize = 4096;

impl SessionWorkingDirectoryTool {
    pub fn new(pool: sqlx::SqlitePool, session_id: String, workspace_root: PathBuf) -> Self {
        Self {
            store: SessionWorkingDirectoryStore::new(pool),
            session_id,
            workspace_root,
        }
    }

    fn canonical_directory(&self, raw: &str) -> Result<String, ToolError> {
        if raw.len() > MAX_SESSION_WORKING_DIRECTORY_BYTES {
            return Err(ToolError::Execution(
                "session working directory exceeds 4096 bytes".into(),
            ));
        }
        let input = Path::new(raw);
        if input.is_absolute()
            || input.components().any(|part| {
                matches!(
                    part,
                    Component::ParentDir | Component::RootDir | Component::Prefix(_)
                )
            })
        {
            return Err(ToolError::Permission(
                "session working directory must stay relative to the session workspace".into(),
            ));
        }
        let root = self
            .workspace_root
            .canonicalize()
            .map_err(|_| ToolError::Execution("session workspace root is unavailable".into()))?;
        let candidate = root.join(input);
        crate::tool::util::check_path_for_symlinks(&candidate)?;
        let canonical = candidate
            .canonicalize()
            .map_err(|_| ToolError::Execution("session working directory does not exist".into()))?;
        if !canonical.starts_with(&root) || !canonical.is_dir() {
            return Err(ToolError::Permission(
                "session working directory must be a directory inside the session workspace".into(),
            ));
        }
        canonical
            .strip_prefix(root)
            .map(|path| {
                if path.as_os_str().is_empty() {
                    ".".into()
                } else {
                    path.to_string_lossy().into_owned()
                }
            })
            .map_err(|_| {
                ToolError::Permission("session working directory escaped its workspace".into())
            })
    }

    async fn execute_inner(&self, input: serde_json::Value) -> Result<String, ToolError> {
        let action = input["action"].as_str().unwrap_or("get");
        let store = &self.store;
        let state = match action {
            "get" => store.get(&self.session_id).await,
            "set" | "reset" => {
                let revision = input["expected_revision"].as_u64().ok_or_else(|| {
                    ToolError::Execution("expected_revision is required for set/reset".into())
                })?;
                let relative_path =
                    if action == "reset" {
                        ".".to_owned()
                    } else {
                        self.canonical_directory(input["path"].as_str().ok_or_else(|| {
                            ToolError::Execution("path is required for set".into())
                        })?)?
                    };
                store
                    .compare_set(&self.session_id, revision, &relative_path)
                    .await
            }
            _ => {
                return Err(ToolError::Execution(
                    "action must be get, set, or reset".into(),
                ))
            }
        }
        .map_err(|error| ToolError::Execution(error.to_string()))?;
        let root = self
            .workspace_root
            .canonicalize()
            .map_err(|_| ToolError::Execution("session workspace root is unavailable".into()))?;
        let current = root.join(&state.relative_path);
        let canonical = current.canonicalize().map_err(|_| {
            ToolError::Execution("stored session working directory is unavailable; reset it".into())
        })?;
        if !canonical.starts_with(&root) || !canonical.is_dir() {
            return Err(ToolError::Permission(
                "stored session working directory is outside the session workspace; reset it"
                    .into(),
            ));
        }
        Ok(json!({
            "relative_path": state.relative_path,
            "absolute_path": canonical,
            "revision": state.revision
        })
        .to_string())
    }
}

#[async_trait]
impl Tool for SessionWorkingDirectoryTool {
    fn name(&self) -> &str {
        "session_working_directory"
    }

    fn description(&self) -> &str {
        "Get, set, or reset the persistent default working directory for this session. Paths must be existing directories inside the original workspace. Changes apply to later turns; in-flight tools keep their captured directory."
    }

    fn parameters(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "action": {"type": "string", "enum": ["get", "set", "reset"]},
                "path": {"type": "string", "maxLength": 4096, "description": "Workspace-relative existing directory for set"},
                "expected_revision": {"type": "integer", "minimum": 0, "description": "Revision returned by get; required for set/reset"}
            },
            "required": ["action"],
            "additionalProperties": false
        })
    }

    fn category(&self) -> ToolCategory {
        ToolCategory::Mutating
    }

    async fn execute(&self, input: serde_json::Value) -> Result<String, ToolError> {
        self.execute_inner(input).await
    }

    async fn execute_structured(
        &self,
        input: serde_json::Value,
        _ctx: Option<ToolExecutionContext>,
    ) -> Result<StructuredToolResult, ToolError> {
        let output = self.execute_inner(input).await?;
        Ok(StructuredToolResult::legacy(self.name(), output))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn session_pool() -> sqlx::SqlitePool {
        use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
        use std::str::FromStr;

        let name = format!("tool_session_cwd_{}", uuid::Uuid::new_v4().simple());
        let options =
            SqliteConnectOptions::from_str(&format!("file:{name}?mode=memory&cache=shared"))
                .unwrap()
                .create_if_missing(true)
                .foreign_keys(true);
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect_with(options)
            .await
            .unwrap();
        codegg_core::session::schema::migrate(&pool).await.unwrap();
        let now = chrono::Utc::now().timestamp_millis();
        sqlx::query("INSERT INTO project(id, worktree, sandboxes, time_created, time_updated) VALUES ('p', '/tmp', '[]', ?, ?)")
            .bind(now)
            .bind(now)
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query("INSERT INTO session(id, project_id, slug, directory, title, version, time_created, time_updated) VALUES ('s', 'p', 's', '/tmp', 's', '1', ?, ?)")
            .bind(now)
            .bind(now)
            .execute(&pool)
            .await
            .unwrap();
        pool
    }

    #[tokio::test]
    async fn session_cwd_accepts_workspace_directory_and_refuses_escape_or_missing_paths() {
        let root = tempfile::tempdir().unwrap();
        let child = root.path().join("crates");
        std::fs::create_dir(&child).unwrap();
        let pool = sqlx::SqlitePool::connect_lazy("sqlite::memory:").unwrap();
        let tool =
            SessionWorkingDirectoryTool::new(pool, "session".to_owned(), root.path().to_path_buf());
        assert_eq!(tool.canonical_directory("crates").unwrap(), "crates");
        assert!(tool.canonical_directory("../outside").is_err());
        assert!(tool.canonical_directory("missing").is_err());
        assert!(tool
            .canonical_directory(root.path().to_str().unwrap())
            .is_err());
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn session_cwd_refuses_symlink_escape() {
        let root = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        std::os::unix::fs::symlink(outside.path(), root.path().join("escape")).unwrap();
        let pool = sqlx::SqlitePool::connect_lazy("sqlite::memory:").unwrap();
        let tool =
            SessionWorkingDirectoryTool::new(pool, "session".to_owned(), root.path().to_path_buf());
        assert!(tool.canonical_directory("escape").is_err());
    }

    #[tokio::test]
    async fn session_cwd_tool_persists_gets_resets_and_rejects_stale_updates() {
        let root = tempfile::tempdir().unwrap();
        std::fs::create_dir(root.path().join("crates")).unwrap();
        let tool = SessionWorkingDirectoryTool::new(
            session_pool().await,
            "s".to_owned(),
            root.path().to_path_buf(),
        );

        let initial: serde_json::Value =
            serde_json::from_str(&tool.execute_inner(json!({"action": "get"})).await.unwrap())
                .unwrap();
        assert_eq!(initial["relative_path"], ".");
        assert_eq!(initial["revision"], 0);
        let changed: serde_json::Value = serde_json::from_str(
            &tool
                .execute_inner(json!({"action": "set", "path": "crates", "expected_revision": 0}))
                .await
                .unwrap(),
        )
        .unwrap();
        assert_eq!(changed["relative_path"], "crates");
        assert_eq!(
            changed["absolute_path"],
            root.path()
                .canonicalize()
                .unwrap()
                .join("crates")
                .to_string_lossy()
                .as_ref()
        );
        assert_eq!(changed["revision"], 1);
        assert!(tool
            .execute_inner(json!({"action": "set", "path": ".", "expected_revision": 0}))
            .await
            .is_err());
        let reset: serde_json::Value = serde_json::from_str(
            &tool
                .execute_inner(json!({"action": "reset", "expected_revision": 1}))
                .await
                .unwrap(),
        )
        .unwrap();
        assert_eq!(reset["relative_path"], ".");
        assert_eq!(reset["revision"], 2);
        assert_eq!(tool.category(), ToolCategory::Mutating);
    }
}

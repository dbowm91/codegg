use sqlx::{Row, SqlitePool};
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::sync::{broadcast, RwLock};
use uuid::Uuid;

const MAX_EXPIRY_SECS: i64 = 3 * 24 * 60 * 60;

#[derive(Debug, Clone, PartialEq)]
pub enum TaskStatus {
    Pending,
    Running,
    Completed,
    Failed,
    Interrupted,
}

impl TaskStatus {
    fn as_str(&self) -> &'static str {
        match self {
            TaskStatus::Pending => "pending",
            TaskStatus::Running => "running",
            TaskStatus::Completed => "completed",
            TaskStatus::Failed => "failed",
            TaskStatus::Interrupted => "interrupted",
        }
    }
}

#[derive(Debug, Clone)]
pub struct BackgroundTask {
    pub id: String,
    pub interval: Duration,
    pub message: String,
    pub last_run: Option<i64>,
    pub created_at: i64,
    pub session_id: String,
    pub db_id: Option<i64>,
}

impl BackgroundTask {
    pub fn new(session_id: String, interval: Duration, message: String) -> Self {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);
        Self {
            id: Uuid::new_v4().to_string(),
            interval,
            message,
            last_run: None,
            created_at: now,
            session_id,
            db_id: None,
        }
    }

    pub fn is_expired(&self) -> bool {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);
        now.saturating_sub(self.created_at) > MAX_EXPIRY_SECS
    }

    pub fn should_fire(&self) -> bool {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);
        if let Some(last) = self.last_run {
            now.saturating_sub(last) >= self.interval.as_secs() as i64
        } else {
            true
        }
    }

    pub fn mark_run(&mut self) {
        self.last_run = Some(
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|d| d.as_secs() as i64)
                .unwrap_or(0),
        );
    }
}

pub type TaskCallback = Arc<dyn Fn(String, String) + Send + Sync>;

pub struct BackgroundScheduler {
    tasks: Arc<RwLock<Vec<BackgroundTask>>>,
    shutdown_tx: broadcast::Sender<()>,
    callback: Option<TaskCallback>,
    pool: Option<SqlitePool>,
}

impl BackgroundScheduler {
    pub fn new() -> Self {
        let (shutdown_tx, _) = broadcast::channel(1);
        Self {
            tasks: Arc::new(RwLock::new(Vec::new())),
            shutdown_tx,
            callback: None,
            pool: None,
        }
    }

    pub fn with_callback(callback: TaskCallback) -> Self {
        let mut scheduler = Self::new();
        scheduler.callback = Some(callback);
        scheduler
    }

    pub fn with_pool(mut self, pool: SqlitePool) -> Self {
        self.pool = Some(pool);
        self
    }

    pub async fn add(&self, mut task: BackgroundTask) -> Result<String, sqlx::Error> {
        let id = task.id.clone();
        if let Some(ref pool) = self.pool {
            match self.save_task(pool, &task).await {
                Ok(db_id) => {
                    task.db_id = Some(db_id);
                    self.tasks.write().await.push(task);
                    Ok(id)
                }
                Err(e) => Err(e),
            }
        } else {
            self.tasks.write().await.push(task);
            Ok(id)
        }
    }

    pub async fn remove(&self, id: &str) -> bool {
        let mut tasks = self.tasks.write().await;
        if let Some(pos) = tasks.iter().position(|t| t.id == id) {
            let task = tasks.remove(pos);
            if let Some(db_id) = task.db_id {
                if let Some(ref pool) = self.pool {
                    let _ = self.mark_task_complete(pool, db_id).await;
                }
            }
            true
        } else {
            false
        }
    }

    pub async fn list(&self) -> Vec<BackgroundTask> {
        self.tasks.read().await.clone()
    }

    pub async fn get(&self, id: &str) -> Option<BackgroundTask> {
        self.tasks.read().await.iter().find(|t| t.id == id).cloned()
    }

    pub async fn cleanup_expired(&self) {
        let mut tasks = self.tasks.write().await;
        tasks.retain(|t| !t.is_expired());
    }

    pub async fn tick(&self) -> Vec<BackgroundTask> {
        let mut tasks = self.tasks.write().await;
        let mut ready = Vec::new();

        for task in tasks.iter_mut() {
            if task.should_fire() && !task.is_expired() {
                task.mark_run();
                ready.push(task.clone());
            }
        }

        tasks.retain(|t| !t.is_expired());
        ready
    }

    pub fn subscribe_shutdown(&self) -> broadcast::Receiver<()> {
        self.shutdown_tx.subscribe()
    }

    pub async fn shutdown(&self) {
        let _ = self.shutdown_tx.send(());
    }

    pub fn invoke_callback(&self, task_id: String, message: String) {
        if let Some(ref cb) = self.callback {
            cb(task_id, message);
        }
    }

    pub fn spawn_loop(
        &self,
        pool: Arc<crate::agent::worker::SubAgentPool>,
        tick_interval: Duration,
    ) {
        let tasks = Arc::clone(&self.tasks);
        let shutdown_rx = self.shutdown_tx.subscribe();
        let pool = Arc::clone(&pool);

        tokio::spawn(async move {
            let mut shutdown_rx = shutdown_rx;
            loop {
                tokio::select! {
                    biased;
                    _ = shutdown_rx.recv() => {
                        tracing::info!("Background task loop shutting down");
                        break;
                    }
                    _ = tokio::time::sleep(tick_interval) => {
                        let ready = {
                            let mut tasks_guard = tasks.write().await;
                            let mut ready = Vec::new();
                            for task in tasks_guard.iter_mut() {
                                if task.should_fire() && !task.is_expired() {
                                    task.mark_run();
                                    ready.push(task.clone());
                                }
                            }
                            tasks_guard.retain(|t| !t.is_expired());
                            ready
                        };

                        for task in ready {
                            let prompt = format!("[Background] {}", task.message);
                            let request = crate::agent::worker::SubAgentRequest {
                                task_id: rand::random::<u64>(),
                                prompt,
                                agent: "build".to_string(),
                                parent_id: Some(task.session_id),
                                denied_tools: Vec::new(),
                                description: "Background loop task".to_string(),
                                depth: 0,
                            };
                            if let Err(e) = pool.spawner().send(request).await {
                                tracing::warn!("Failed to dispatch background task: {}", e);
                            }
                        }
                    }
                }
            }
        });
    }

    pub async fn load_tasks(&self, pool: &SqlitePool) -> Result<(), sqlx::Error> {
        let rows = sqlx::query(
            r#"
            SELECT id, parent_id, session_id, description, prompt, agent, status,
                   result, denied_tools, time_created, time_updated
            FROM task
            WHERE status IN ('pending', 'running')
            "#,
        )
        .fetch_all(pool)
        .await?;

        let mut tasks = self.tasks.write().await;
        for row in rows {
            let db_id: i64 = row.get("id");
            let session_id: String = row.get("session_id");
            let description: String = row.get("description");
            let prompt: String = row.get("prompt");
            let status: String = row.get("status");
            let time_created: i64 = row.get("time_created");

            if status == "running" {
                tracing::info!("Marking interrupted task {} as interrupted", db_id);
                let _ = sqlx::query("UPDATE task SET status = 'interrupted' WHERE id = ?")
                    .bind(db_id)
                    .execute(pool)
                    .await;
            }

            let interval = parse_duration(&prompt).unwrap_or_else(|| Duration::from_secs(3600));

            let task = BackgroundTask {
                id: row
                    .try_get::<Option<String>, _>("parent_id")
                    .ok()
                    .flatten()
                    .unwrap_or_else(|| Uuid::new_v4().to_string()),
                interval,
                message: description,
                last_run: Some(time_created),
                created_at: time_created,
                session_id,
                db_id: Some(db_id),
            };

            tasks.push(task);
        }

        tracing::info!("Loaded {} background tasks from database", tasks.len());
        Ok(())
    }

    pub async fn save_task(
        &self,
        pool: &SqlitePool,
        task: &BackgroundTask,
    ) -> Result<i64, sqlx::Error> {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);

        let interval_secs = task.interval.as_secs() as i64;
        let prompt = format!("{}s", interval_secs);

        let result = sqlx::query(
            r#"
            INSERT INTO task (parent_id, session_id, description, prompt, agent, status,
                           result, denied_tools, time_created, time_updated)
            VALUES (?, ?, ?, ?, 'background', 'pending', NULL, NULL, ?, ?)
            "#,
        )
        .bind(&task.id)
        .bind(&task.session_id)
        .bind(&task.message)
        .bind(&prompt)
        .bind(task.created_at)
        .bind(now)
        .execute(pool)
        .await?;

        Ok(result.last_insert_rowid())
    }

    pub async fn update_task_status(
        &self,
        pool: &SqlitePool,
        db_id: i64,
        status: TaskStatus,
    ) -> Result<(), sqlx::Error> {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);

        sqlx::query("UPDATE task SET status = ?, time_updated = ? WHERE id = ?")
            .bind(status.as_str())
            .bind(now)
            .bind(db_id)
            .execute(pool)
            .await?;

        Ok(())
    }

    pub async fn mark_task_complete(
        &self,
        pool: &SqlitePool,
        db_id: i64,
    ) -> Result<(), sqlx::Error> {
        self.update_task_status(pool, db_id, TaskStatus::Completed)
            .await
    }
}

impl Default for BackgroundScheduler {
    fn default() -> Self {
        Self::new()
    }
}

pub fn parse_duration(s: &str) -> Option<Duration> {
    let s = s.trim();
    if let Some(rest) = s.strip_suffix('s') {
        rest.parse::<u64>().ok().map(Duration::from_secs)
    } else if let Some(rest) = s.strip_suffix("min") {
        rest.parse::<u64>()
            .ok()
            .map(|n| Duration::from_secs(n * 60))
    } else if let Some(rest) = s.strip_suffix('m') {
        rest.parse::<u64>()
            .ok()
            .map(|n| Duration::from_secs(n * 60))
    } else if let Some(rest) = s.strip_suffix('h') {
        rest.parse::<u64>()
            .ok()
            .map(|n| Duration::from_secs(n * 3600))
    } else if let Some(rest) = s.strip_suffix('d') {
        rest.parse::<u64>()
            .ok()
            .map(|n| Duration::from_secs(n * 86400))
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_duration_seconds() {
        assert_eq!(parse_duration("30s"), Some(Duration::from_secs(30)));
        assert_eq!(parse_duration("120s"), Some(Duration::from_secs(120)));
    }

    #[test]
    fn test_parse_duration_minutes() {
        assert_eq!(parse_duration("5m"), Some(Duration::from_secs(300)));
        assert_eq!(parse_duration("5min"), Some(Duration::from_secs(300)));
        assert_eq!(parse_duration("1m"), Some(Duration::from_secs(60)));
    }

    #[test]
    fn test_parse_duration_hours() {
        assert_eq!(parse_duration("1h"), Some(Duration::from_secs(3600)));
        assert_eq!(parse_duration("24h"), Some(Duration::from_secs(86400)));
    }

    #[test]
    fn test_parse_duration_days() {
        assert_eq!(parse_duration("1d"), Some(Duration::from_secs(86400)));
        assert_eq!(parse_duration("3d"), Some(Duration::from_secs(259200)));
    }

    #[test]
    fn test_parse_duration_invalid() {
        assert_eq!(parse_duration("abc"), None);
        assert_eq!(parse_duration(""), None);
        assert_eq!(parse_duration("5x"), None);
    }

    #[test]
    fn test_background_task_should_fire() {
        let task = BackgroundTask::new(
            "session1".to_string(),
            Duration::from_secs(60),
            "test".to_string(),
        );
        assert!(task.should_fire());
    }
}

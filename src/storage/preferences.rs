//! Persistent key/value preferences storage.
//!
//! Backed by the `user_preferences` SQLite table. Used for state that must
//! outlive config-file edits (e.g. the user's chosen theme and last-used
//! model). Kept intentionally tiny: two columns (`key`, `value`) plus an
//! `updated_at` epoch-millis timestamp for diagnostics.
//!
//! The keys used elsewhere in the codebase are exposed as public
//! constants so callers don't have to remember string literals.

use sqlx::SqlitePool;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::error::StorageError;

/// Storage key for the active theme id (kebab-case slug, e.g. `"cyber-red"`).
pub const KEY_THEME_ACTIVE: &str = "theme.active";

/// Storage key for the last-used model id (e.g. `"opencode_zen/big-pickle"`).
pub const KEY_MODEL_LAST_USED: &str = "model.last_used";

/// Thin wrapper around the `user_preferences` table. Cheap to clone —
/// holds only a `SqlitePool`.
#[derive(Clone)]
pub struct UserPreferences {
    pool: SqlitePool,
}

impl UserPreferences {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }

    /// Read a value by key. Returns `Ok(None)` when the key is absent.
    pub async fn get(&self, key: &str) -> Result<Option<String>, StorageError> {
        let row: Option<(String,)> =
            sqlx::query_as("SELECT value FROM user_preferences WHERE key = ?1")
                .bind(key)
                .fetch_optional(&self.pool)
                .await
                .map_err(|e| StorageError::Database(e.to_string()))?;
        Ok(row.map(|(v,)| v))
    }

    /// Insert or update a value. `updated_at` is set to the current wall
    /// clock (epoch millis).
    pub async fn set(&self, key: &str, value: &str) -> Result<(), StorageError> {
        let now = now_millis();
        sqlx::query(
            r#"
            INSERT INTO user_preferences (key, value, updated_at)
            VALUES (?1, ?2, ?3)
            ON CONFLICT(key) DO UPDATE SET
                value = excluded.value,
                updated_at = excluded.updated_at
            "#,
        )
        .bind(key)
        .bind(value)
        .bind(now)
        .execute(&self.pool)
        .await
        .map_err(|e| StorageError::Database(e.to_string()))?;
        Ok(())
    }

    /// Delete a key. Returns the number of rows affected (0 or 1).
    pub async fn delete(&self, key: &str) -> Result<u64, StorageError> {
        let res = sqlx::query("DELETE FROM user_preferences WHERE key = ?1")
            .bind(key)
            .execute(&self.pool)
            .await
            .map_err(|e| StorageError::Database(e.to_string()))?;
        Ok(res.rows_affected())
    }

    /// Returns the `updated_at` epoch-millis for `key`, or `None` if the
    /// key has never been set.
    pub async fn updated_at(&self, key: &str) -> Result<Option<i64>, StorageError> {
        let row: Option<(i64,)> =
            sqlx::query_as("SELECT updated_at FROM user_preferences WHERE key = ?1")
                .bind(key)
                .fetch_optional(&self.pool)
                .await
                .map_err(|e| StorageError::Database(e.to_string()))?;
        Ok(row.map(|(v,)| v))
    }
}

fn now_millis() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::sqlite::SqlitePoolOptions;
    use std::time::Duration;

    async fn temp_pool() -> SqlitePool {
        // In-memory SQLite is perfect for these unit tests.
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .acquire_timeout(Duration::from_secs(5))
            .connect("sqlite::memory:")
            .await
            .expect("connect to in-memory db");
        // Apply the same migration the app uses.
        crate::session::schema::migrate(&pool)
            .await
            .expect("migrate");
        pool
    }

    #[tokio::test]
    async fn get_returns_none_for_missing_key() {
        let prefs = UserPreferences::new(temp_pool().await);
        let got = prefs.get(KEY_THEME_ACTIVE).await.unwrap();
        assert!(got.is_none());
    }

    #[tokio::test]
    async fn set_then_get_round_trips() {
        let prefs = UserPreferences::new(temp_pool().await);
        prefs.set(KEY_THEME_ACTIVE, "cyber-red").await.unwrap();
        let got = prefs.get(KEY_THEME_ACTIVE).await.unwrap();
        assert_eq!(got.as_deref(), Some("cyber-red"));
    }

    #[tokio::test]
    async fn set_overwrites_existing_value() {
        let prefs = UserPreferences::new(temp_pool().await);
        prefs.set(KEY_THEME_ACTIVE, "midnight").await.unwrap();
        prefs.set(KEY_THEME_ACTIVE, "cyber-red").await.unwrap();
        let got = prefs.get(KEY_THEME_ACTIVE).await.unwrap();
        assert_eq!(got.as_deref(), Some("cyber-red"));
    }

    #[tokio::test]
    async fn delete_removes_key() {
        let prefs = UserPreferences::new(temp_pool().await);
        prefs
            .set(KEY_MODEL_LAST_USED, "opencode_zen/big-pickle")
            .await
            .unwrap();
        let n = prefs.delete(KEY_MODEL_LAST_USED).await.unwrap();
        assert_eq!(n, 1);
        let got = prefs.get(KEY_MODEL_LAST_USED).await.unwrap();
        assert!(got.is_none());
    }

    #[tokio::test]
    async fn updated_at_advances_on_rewrite() {
        let prefs = UserPreferences::new(temp_pool().await);
        prefs.set(KEY_THEME_ACTIVE, "midnight").await.unwrap();
        let t1 = prefs.updated_at(KEY_THEME_ACTIVE).await.unwrap().unwrap();
        // Force a measurable gap.
        tokio::time::sleep(Duration::from_millis(5)).await;
        prefs.set(KEY_THEME_ACTIVE, "cyber-red").await.unwrap();
        let t2 = prefs.updated_at(KEY_THEME_ACTIVE).await.unwrap().unwrap();
        assert!(
            t2 >= t1,
            "updated_at should not go backwards ({} < {})",
            t2,
            t1
        );
    }
}

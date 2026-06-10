use crate::error::StorageError;
use crate::{ModelInfo, ModelVariant, ProviderRegistry};

use sqlx::SqlitePool;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::RwLock;

pub struct ModelDiscoveryService {
    models: Arc<RwLock<Vec<ModelInfoInternal>>>,
    last_refresh: Arc<RwLock<Option<Instant>>>,
    #[allow(dead_code)]
    cache_path: PathBuf,
    ttl: Duration,
    pool: Option<SqlitePool>,
}

impl ModelDiscoveryService {
    pub fn new(cache_path: PathBuf) -> Self {
        Self {
            models: Arc::new(RwLock::new(Vec::new())),
            last_refresh: Arc::new(RwLock::new(None)),
            cache_path,
            ttl: Duration::from_secs(3600),
            pool: None,
        }
    }

    pub fn with_pool(mut self, pool: SqlitePool) -> Self {
        self.pool = Some(pool);
        self
    }

    pub async fn initialize(&self) {
        self.load_from_cache_or_embedded().await;
    }

    async fn load_from_cache_or_embedded(&self) {
        if let Some(ref pool) = self.pool {
            if let Ok(models) = self.load_from_db(pool).await {
                if !models.is_empty() {
                    let mut m = self.models.write().await;
                    *m = models.clone();

                    let latest = models.iter().map(|m| m.fetched_at).max();
                    if let Some(timestamp) = latest {
                        let mut last = self.last_refresh.write().await;
                        *last = Some(
                            Instant::now()
                                - Duration::from_secs(
                                    (now_secs().saturating_sub(timestamp)) as u64,
                                ),
                        );
                    }
                    return;
                }
            }
        }

        let mut models = self.models.write().await;
        *models = crate::models::embedded_models()
            .into_iter()
            .map(|m| ModelInfoInternal {
                id: m.id,
                provider: m.provider,
                name: m.name,
                context_window: m.context_window,
                max_output_tokens: m.max_output_tokens,
                supports_tools: m.supports_tools,
                supports_vision: m.supports_vision,
                variants: m.variants,
                fetched_at: now_secs(),
            })
            .collect();
    }

    async fn load_from_db(
        &self,
        pool: &SqlitePool,
    ) -> Result<Vec<ModelInfoInternal>, StorageError> {
        #[allow(clippy::type_complexity)]
        let rows: Vec<(
            String,
            String,
            String,
            Option<i64>,
            Option<i64>,
            bool,
            bool,
            i64,
        )> = sqlx::query_as(
            "SELECT id, provider, name, context_window, max_output_tokens, supports_tools, supports_vision, fetched_at FROM cached_models"
        )
        .fetch_all(pool)
        .await
        .map_err(|e| StorageError::Database(e.to_string()))?;

        Ok(rows
            .into_iter()
            .map(|r| ModelInfoInternal {
                id: r.0,
                provider: r.1,
                name: r.2,
                context_window: r.3.unwrap_or(128_000) as usize,
                max_output_tokens: r.4.map(|v| v as usize),
                supports_tools: r.5,
                supports_vision: r.6,
                variants: vec![],
                fetched_at: r.7,
            })
            .collect())
    }

    pub async fn needs_refresh(&self) -> bool {
        let last = self.last_refresh.read().await;
        last.map(|t| t.elapsed() > self.ttl).unwrap_or(true)
    }

    pub async fn refresh(&self, registry: &ProviderRegistry) -> Vec<ModelInfo> {
        let mut all_models = Vec::new();
        let providers = registry.list();

        for (i, provider) in providers.iter().enumerate() {
            if i > 0 {
                tokio::time::sleep(Duration::from_millis(500)).await;
            }

            match provider.discover_models().await {
                Ok(models) => {
                    tracing::debug!(
                        "discovered {} models from provider {}",
                        models.len(),
                        provider.name()
                    );
                    all_models.extend(models);
                }
                Err(e) => {
                    tracing::warn!(
                        "failed to discover models from provider {}: {}",
                        provider.name(),
                        e
                    );
                }
            }
        }

        if all_models.is_empty() {
            all_models = crate::models::embedded_models();
        }

        let now = now_secs();
        let internal_models: Vec<ModelInfoInternal> = all_models
            .iter()
            .cloned()
            .map(|m| ModelInfoInternal {
                id: m.id,
                provider: m.provider,
                name: m.name,
                context_window: m.context_window,
                max_output_tokens: m.max_output_tokens,
                supports_tools: m.supports_tools,
                supports_vision: m.supports_vision,
                variants: m.variants,
                fetched_at: now,
            })
            .collect();

        if let Some(ref pool) = self.pool {
            if let Err(e) = self.save_to_db(pool, &internal_models).await {
                tracing::warn!("failed to cache models: {}", e);
            }
        }

        {
            let mut models = self.models.write().await;
            *models = internal_models;
        }
        {
            let mut last = self.last_refresh.write().await;
            *last = Some(Instant::now());
        }

        all_models
    }

    async fn save_to_db(
        &self,
        pool: &SqlitePool,
        models: &[ModelInfoInternal],
    ) -> Result<(), StorageError> {
        sqlx::query("DELETE FROM cached_models")
            .execute(pool)
            .await
            .map_err(|e| StorageError::Database(e.to_string()))?;

        if models.is_empty() {
            return Ok(());
        }

        use sqlx::query_builder::QueryBuilder;
        let mut query_builder: QueryBuilder<sqlx::Sqlite> = QueryBuilder::new(
            "INSERT INTO cached_models (id, provider, name, context_window, max_output_tokens, supports_tools, supports_vision, fetched_at) ",
        );

        query_builder.push_values(models, |mut b, model| {
            b.push_bind(&model.id)
                .push_bind(&model.provider)
                .push_bind(&model.name)
                .push_bind(model.context_window as i64)
                .push_bind(model.max_output_tokens.map(|v| v as i64))
                .push_bind(model.supports_tools)
                .push_bind(model.supports_vision)
                .push_bind(model.fetched_at);
        });

        query_builder
            .build()
            .execute(pool)
            .await
            .map_err(|e| StorageError::Database(e.to_string()))?;

        Ok(())
    }

    pub async fn get_models(&self) -> Vec<ModelInfo> {
        self.models
            .read()
            .await
            .iter()
            .cloned()
            .map(|m| ModelInfo {
                id: m.id,
                provider: m.provider,
                name: m.name,
                context_window: m.context_window,
                max_output_tokens: m.max_output_tokens,
                supports_tools: m.supports_tools,
                supports_vision: m.supports_vision,
                variants: m.variants,
            })
            .collect()
    }

    pub async fn get_model_ids(&self) -> Vec<String> {
        let models = self.models.read().await;
        models
            .iter()
            .map(|m| format!("{}/{}", m.provider, m.id))
            .collect()
    }

    pub async fn clear_cache(&self) {
        {
            let mut models = self.models.write().await;
            *models = Vec::new();
        }
        {
            let mut last = self.last_refresh.write().await;
            *last = None;
        }

        if let Some(ref pool) = self.pool {
            let _ = sqlx::query("DELETE FROM cached_models").execute(pool).await;
        }
    }
}

#[derive(Clone)]
struct ModelInfoInternal {
    pub id: String,
    pub provider: String,
    pub name: String,
    pub context_window: usize,
    pub max_output_tokens: Option<usize>,
    pub supports_tools: bool,
    pub supports_vision: bool,
    pub variants: Vec<ModelVariant>,
    pub fetched_at: i64,
}

fn now_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64
}

impl Default for ModelDiscoveryService {
    fn default() -> Self {
        Self::new(PathBuf::new())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_needs_refresh_initially_true() {
        let service = ModelDiscoveryService::default();
        service.initialize().await;
        assert!(service.needs_refresh().await);
    }

    #[tokio::test]
    async fn test_get_model_ids_format() {
        let service = ModelDiscoveryService::default();
        service.initialize().await;
        let registry = crate::ProviderRegistry::new();
        service.refresh(&registry).await;
        let ids = service.get_model_ids().await;
        for id in ids {
            assert!(id.contains('/'));
        }
    }
}

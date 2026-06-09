use crate::ModelInfo;
use std::collections::HashMap;
use std::time::{Duration, Instant};

#[derive(Debug, Clone)]
pub struct ModelCatalog {
    models: HashMap<String, ModelInfo>,
    last_fetch: Option<Instant>,
    cache_ttl: Duration,
}

impl Default for ModelCatalog {
    fn default() -> Self {
        Self::new()
    }
}

impl ModelCatalog {
    pub fn new() -> Self {
        let mut catalog = Self {
            models: HashMap::new(),
            last_fetch: None,
            cache_ttl: Duration::from_secs(3600),
        };
        catalog.seed_embedded();
        catalog
    }

    fn seed_embedded(&mut self) {
        for model in crate::models::embedded_models() {
            self.models.insert(model.id.clone(), model);
        }
    }

    pub async fn fetch_live(&mut self) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(10))
            .build()?;

        let resp = client.get("https://models.dev/api/models").send().await?;

        if resp.status().is_success() {
            let body: serde_json::Value = resp.json().await?;
            if let Some(models_arr) = body.as_array() {
                for entry in models_arr {
                    if let (Some(id), Some(name), Some(provider)) = (
                        entry.get("id").and_then(|v| v.as_str()),
                        entry.get("name").and_then(|v| v.as_str()),
                        entry.get("provider").and_then(|v| v.as_str()),
                    ) {
                        let ctx = entry
                            .get("context_window")
                            .and_then(|v| v.as_u64())
                            .unwrap_or(128_000) as usize;
                        let max_out = entry
                            .get("max_output_tokens")
                            .and_then(|v| v.as_u64())
                            .map(|v| v as usize);
                        let tools = entry
                            .get("supports_tools")
                            .and_then(|v| v.as_bool())
                            .unwrap_or(true);
                        let vision = entry
                            .get("supports_vision")
                            .and_then(|v| v.as_bool())
                            .unwrap_or(false);

                        self.models.insert(
                            id.to_string(),
                            ModelInfo {
                                id: id.to_string(),
                                name: name.to_string(),
                                provider: provider.to_string(),
                                context_window: ctx,
                                max_output_tokens: max_out,
                                supports_tools: tools,
                                supports_vision: vision,
                                variants: Vec::new(),
                            },
                        );
                    }
                }
            }
            self.last_fetch = Some(Instant::now());
        }

        Ok(())
    }

    pub fn needs_refresh(&self) -> bool {
        self.last_fetch
            .map(|t| t.elapsed() > self.cache_ttl)
            .unwrap_or(true)
    }

    pub fn get(&self, id: &str) -> Option<&ModelInfo> {
        self.models.get(id)
    }

    pub fn all(&self) -> Vec<&ModelInfo> {
        self.models.values().collect()
    }

    pub fn merge(&mut self, models: Vec<ModelInfo>) {
        for m in models {
            self.models.entry(m.id.clone()).or_insert(m);
        }
    }
}

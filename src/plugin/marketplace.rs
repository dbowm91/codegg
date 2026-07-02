use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum PluginTier {
    Official,
    Repository,
    Personal,
}

impl std::fmt::Display for PluginTier {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PluginTier::Official => write!(f, "official"),
            PluginTier::Repository => write!(f, "repository"),
            PluginTier::Personal => write!(f, "personal"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MarketplacePlugin {
    pub id: String,
    pub name: String,
    pub version: String,
    pub description: Option<String>,
    pub author: Option<String>,
    pub homepage: Option<String>,
    pub tier: PluginTier,
    pub hooks: Vec<String>,
}

pub struct MarketplaceService {
    plugins_dir: PathBuf,
}

impl MarketplaceService {
    pub fn new() -> Self {
        Self {
            plugins_dir: crate::plugin::install::plugins_dir(),
        }
    }

    pub fn plugins_dir(&self) -> &PathBuf {
        &self.plugins_dir
    }

    pub async fn list_local_plugins(&self) -> Vec<MarketplacePlugin> {
        let mut plugins = Vec::new();
        let mut read_dir = match tokio::fs::read_dir(&self.plugins_dir).await {
            Ok(d) => d,
            Err(_) => return plugins,
        };

        while let Some(entry_result) = read_dir.next_entry().await.unwrap_or(None) {
            let path = entry_result.path();
            if path.is_dir() {
                let manifest_path = path.join("manifest.toml");
                if manifest_path.exists() {
                    if let Ok(content) = tokio::fs::read_to_string(&manifest_path).await {
                        if let Ok(manifest) =
                            toml::from_str::<crate::plugin::manifest::PluginManifest>(&content)
                        {
                            plugins.push(MarketplacePlugin {
                                id: manifest.name.clone(),
                                name: manifest.name,
                                version: manifest.version,
                                description: manifest.description,
                                author: manifest.author,
                                homepage: manifest.homepage,
                                tier: PluginTier::Personal,
                                hooks: manifest.hooks.iter().map(|h| h.hook_type.clone()).collect(),
                            });
                        }
                    }
                }
            }
        }
        plugins
    }

    pub async fn search_plugins(&self, query: &str) -> Vec<MarketplacePlugin> {
        let all = self.list_local_plugins().await;
        let query_lower = query.to_lowercase();
        all.into_iter()
            .filter(|p| {
                p.name.to_lowercase().contains(&query_lower)
                    || p.description
                        .as_ref()
                        .map(|d| d.to_lowercase().contains(&query_lower))
                        .unwrap_or(false)
            })
            .collect()
    }

    pub fn list_official_plugins() -> Vec<MarketplacePlugin> {
        Vec::new()
    }

    pub fn list_repository_plugins() -> Vec<MarketplacePlugin> {
        Vec::new()
    }
}

impl Default for MarketplaceService {
    fn default() -> Self {
        Self::new()
    }
}

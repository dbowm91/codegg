//! Bounded extension catalog discovery and explicit host-mediated install.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::sync::RwLock;

const MAX_CATALOG_BYTES: usize = 2 * 1024 * 1024;
const MAX_ENTRIES: usize = 256;
const MAX_PACKAGE_BYTES: usize = 64 * 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
pub enum PluginTier {
    #[default]
    Official,
    Repository,
    Personal,
}

impl std::fmt::Display for PluginTier {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}",
            match self {
                Self::Official => "official",
                Self::Repository => "repository",
                Self::Personal => "personal",
            }
        )
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ExtensionPackageFormat {
    Codegg,
    AgentPluginsV1,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExtensionArtifact {
    pub path: Option<String>,
    pub url: Option<String>,
    pub sha256: Option<String>,
    pub media_type: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExtensionCatalogEntry {
    pub id: String,
    pub name: String,
    pub version: String,
    pub description: String,
    pub author: Option<String>,
    pub homepage: Option<String>,
    pub repository: Option<String>,
    pub license: Option<String>,
    pub format: ExtensionPackageFormat,
    pub artifact: ExtensionArtifact,
    #[serde(default)]
    pub components: Vec<String>,
    #[serde(default)]
    pub prerequisites: Vec<String>,
    #[serde(default)]
    pub security_notes: Vec<String>,
    #[serde(skip)]
    pub source_id: String,
    #[serde(skip)]
    pub tier: PluginTier,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExtensionCatalogSource {
    pub id: String,
    pub tier: PluginTier,
    pub location: String,
}

#[derive(Debug, Deserialize)]
struct CatalogFile {
    schema_version: u32,
    entries: Vec<ExtensionCatalogEntry>,
}

#[derive(Debug, thiserror::Error)]
pub enum CatalogError {
    #[error("catalog source is too large")]
    TooLarge,
    #[error("catalog parse failed: {0}")]
    Parse(String),
    #[error("catalog entry not found: {0}@{1}")]
    EntryNotFound(String, String),
    #[error("catalog artifact is invalid: {0}")]
    Artifact(String),
    #[error("catalog download failed: {0}")]
    Download(String),
    #[error("package install failed: {0}")]
    Install(String),
}

#[derive(Clone)]
pub struct MarketplaceService {
    plugins_dir: PathBuf,
    catalog: Arc<RwLock<Vec<ExtensionCatalogEntry>>>,
}

impl MarketplaceService {
    pub fn new() -> Self {
        let service = Self {
            plugins_dir: crate::plugin::install::plugins_dir(),
            catalog: Arc::new(RwLock::new(Vec::new())),
        };
        if let Ok(entries) = parse_catalog(
            include_str!("../../assets/extension-catalog.json"),
            "bundled",
            PluginTier::Official,
        ) {
            if let Ok(mut catalog) = service.catalog.try_write() {
                *catalog = entries;
            }
        }
        service
    }

    pub fn plugins_dir(&self) -> &PathBuf {
        &self.plugins_dir
    }

    pub async fn load_catalog_json(
        &self,
        source: ExtensionCatalogSource,
        raw: &str,
    ) -> Result<usize, CatalogError> {
        if raw.len() > MAX_CATALOG_BYTES {
            return Err(CatalogError::TooLarge);
        }
        let entries = parse_catalog(raw, &source.id, source.tier)?;
        let count = entries.len();
        let mut catalog = self.catalog.write().await;
        catalog.retain(|entry| entry.source_id != source.id);
        catalog.extend(entries);
        catalog.sort_by(|a, b| {
            a.id.cmp(&b.id)
                .then(a.version.cmp(&b.version))
                .then(a.source_id.cmp(&b.source_id))
        });
        Ok(count)
    }

    pub async fn load_catalog_file(
        &self,
        source: ExtensionCatalogSource,
        path: &Path,
    ) -> Result<usize, CatalogError> {
        let raw = tokio::fs::read_to_string(path)
            .await
            .map_err(|e| CatalogError::Parse(e.to_string()))?;
        self.load_catalog_json(source, &raw).await
    }

    pub async fn catalog_entries(&self) -> Vec<ExtensionCatalogEntry> {
        self.catalog.read().await.clone()
    }

    pub async fn search_extensions(
        &self,
        query: &str,
        component: Option<&str>,
        limit: usize,
    ) -> Vec<ExtensionCatalogEntry> {
        let query = query.to_lowercase();
        let mut entries: Vec<_> = self
            .catalog
            .read()
            .await
            .iter()
            .filter(|entry| {
                (query.is_empty()
                    || entry.name.to_lowercase().contains(&query)
                    || entry.description.to_lowercase().contains(&query)
                    || entry
                        .components
                        .iter()
                        .any(|c| c.to_lowercase().contains(&query)))
                    && component.is_none_or(|wanted| entry.components.iter().any(|c| c == wanted))
            })
            .cloned()
            .collect();
        entries.truncate(limit.min(16));
        entries
    }

    /// Explicit host action. Model tools never call this method.
    pub async fn install_entry(
        &self,
        id: &str,
        version: &str,
        dest_root: &Path,
    ) -> Result<PathBuf, CatalogError> {
        let entry = self
            .catalog
            .read()
            .await
            .iter()
            .find(|entry| entry.id == id && entry.version == version)
            .cloned()
            .ok_or_else(|| CatalogError::EntryNotFound(id.into(), version.into()))?;
        let package_root = if let Some(path) = entry.artifact.path.as_deref() {
            let root = PathBuf::from(path);
            let root = if root.is_absolute() {
                root
            } else {
                PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(root)
            };
            root.canonicalize()
                .map_err(|e| CatalogError::Artifact(e.to_string()))?
        } else if let Some(url) = entry.artifact.url.as_deref() {
            self.stage_remote(url, entry.artifact.sha256.as_deref())
                .await?
        } else {
            return Err(CatalogError::Artifact("entry has no path or URL".into()));
        };
        let loaded = crate::plugin::package::detect_and_load(&package_root)
            .map_err(CatalogError::Artifact)?;
        if loaded.manifest.name != entry.name || loaded.manifest.version != entry.version {
            return Err(CatalogError::Artifact(
                "package identity/version does not match catalog entry".into(),
            ));
        }
        crate::plugin::install::install_from_path_into(&package_root, dest_root)
            .await
            .map_err(|e| CatalogError::Install(e.to_string()))
    }

    async fn stage_remote(
        &self,
        url: &str,
        expected_sha256: Option<&str>,
    ) -> Result<PathBuf, CatalogError> {
        if !url.starts_with("https://") {
            return Err(CatalogError::Artifact(
                "remote catalog artifacts require HTTPS".into(),
            ));
        }
        let client =
            crate::http_client::ordinary_http_client_builder(eggfetch_core::Timeout::from_secs(30))
                .build();
        let mut response = client
            .get(url)
            .map_err(|e| CatalogError::Download(e.to_string()))?
            .send()
            .await
            .map_err(|e| CatalogError::Download(e.to_string()))?;
        if !response.status().is_success() {
            return Err(CatalogError::Download(format!(
                "HTTP {}",
                response.status()
            )));
        }
        let bytes = response
            .bytes()
            .await
            .map_err(|e| CatalogError::Download(e.to_string()))?;
        if bytes.len() > MAX_PACKAGE_BYTES {
            return Err(CatalogError::TooLarge);
        }
        if let Some(expected) = expected_sha256 {
            if !format!("{:x}", Sha256::digest(&bytes)).eq_ignore_ascii_case(expected) {
                return Err(CatalogError::Artifact("artifact SHA-256 mismatch".into()));
            }
        }
        let temp = tempfile::tempdir().map_err(|e| CatalogError::Download(e.to_string()))?;
        let archive = temp.path().join("package.tar.gz");
        std::fs::write(&archive, bytes).map_err(|e| CatalogError::Download(e.to_string()))?;
        crate::plugin::install::extract_plugin_archive(&archive, temp.path())
            .map_err(|e| CatalogError::Download(e.to_string()))?;
        if crate::plugin::package::detect_and_load(temp.path()).is_ok() {
            let path = temp.path().to_path_buf();
            std::mem::forget(temp);
            return Ok(path);
        }
        let mut children = std::fs::read_dir(temp.path())
            .map_err(|e| CatalogError::Download(e.to_string()))?
            .filter_map(Result::ok)
            .map(|e| e.path())
            .filter(|p| p.is_dir());
        let child = children
            .next()
            .ok_or_else(|| CatalogError::Artifact("archive contains no package root".into()))?;
        if children.next().is_some() || crate::plugin::package::detect_and_load(&child).is_err() {
            return Err(CatalogError::Artifact(
                "archive must contain exactly one supported package root".into(),
            ));
        }
        std::mem::forget(temp);
        Ok(child)
    }

    pub async fn list_local_plugins(&self) -> Vec<MarketplacePlugin> {
        let mut plugins = Vec::new();
        let Ok(mut read_dir) = tokio::fs::read_dir(&self.plugins_dir).await else {
            return plugins;
        };
        while let Ok(Some(entry)) = read_dir.next_entry().await {
            if !entry.path().is_dir() {
                continue;
            }
            if let Ok(package) = crate::plugin::package::detect_and_load(&entry.path()) {
                plugins.push(MarketplacePlugin {
                    id: package.manifest.name.clone(),
                    name: package.manifest.name,
                    version: package.manifest.version,
                    description: package.manifest.description,
                    author: package.manifest.author,
                    homepage: package.manifest.homepage,
                    tier: PluginTier::Personal,
                    hooks: package
                        .manifest
                        .hooks
                        .iter()
                        .map(|h| h.hook_type.clone())
                        .collect(),
                });
            }
        }
        plugins
    }

    pub async fn search_plugins(&self, query: &str) -> Vec<MarketplacePlugin> {
        let query = query.to_lowercase();
        self.list_local_plugins()
            .await
            .into_iter()
            .filter(|p| {
                p.name.to_lowercase().contains(&query)
                    || p.description
                        .as_deref()
                        .is_some_and(|d| d.to_lowercase().contains(&query))
            })
            .collect()
    }
}

fn parse_catalog(
    raw: &str,
    source_id: &str,
    tier: PluginTier,
) -> Result<Vec<ExtensionCatalogEntry>, CatalogError> {
    let file: CatalogFile =
        serde_json::from_str(raw).map_err(|e| CatalogError::Parse(e.to_string()))?;
    if file.schema_version != 1 {
        return Err(CatalogError::Parse("unsupported catalog schema".into()));
    }
    if file.entries.len() > MAX_ENTRIES {
        return Err(CatalogError::TooLarge);
    }
    let mut entries = Vec::new();
    for mut entry in file.entries {
        if entry.id.is_empty()
            || entry.id.len() > 128
            || entry.name.is_empty()
            || entry.version.is_empty()
            || entry.description.len() > 4096
            || entry.components.len() > 32
            || entry.prerequisites.len() > 32
            || entry.security_notes.len() > 32
        {
            return Err(CatalogError::Parse("catalog entry exceeds bounds".into()));
        }
        if entry.artifact.path.is_none() && entry.artifact.url.is_none() {
            return Err(CatalogError::Parse(format!(
                "entry '{}' has no artifact",
                entry.id
            )));
        }
        entry.source_id = source_id.into();
        entry.tier = tier;
        entries.push(entry);
    }
    Ok(entries)
}

impl Default for MarketplaceService {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test(flavor = "current_thread")]
    async fn bundled_catalog_is_bounded_and_searchable() {
        let service = MarketplaceService::new();
        let entries = service.search_extensions("playwright", None, 16).await;
        assert_eq!(entries.len(), 2);
        assert!(entries.iter().all(|entry| entry.source_id == "bundled"));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn host_install_verifies_bundled_package_identity() {
        let service = MarketplaceService::new();
        let destination = tempfile::tempdir().unwrap();
        let installed = service
            .install_entry("playwright-browser-testing", "1.0.0", destination.path())
            .await
            .unwrap();
        assert!(installed.join("plugin.json").is_file());
        assert!(installed
            .join("skills/playwright-browser-testing/SKILL.md")
            .is_file());
    }
}

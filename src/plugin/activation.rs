//! Durable, context-aware plugin activation.
//!
//! Installation remains owned by [`PluginRegistry`]. This module owns the
//! separate user-scoped selection state used to decide which installed
//! plugins are visible for a global or workspace context. Resolved views are
//! immutable so a turn can pin activation for its lifetime.

use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use tokio::sync::Mutex;

use super::registry::{PluginInfo, PluginInstallKind};

const ACTIVATION_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "workspace_id", rename_all = "snake_case")]
pub enum PluginActivationScope {
    Global,
    Workspace(String),
}

impl PluginActivationScope {
    pub fn label(&self) -> String {
        match self {
            Self::Global => "global".to_string(),
            Self::Workspace(id) => format!("workspace:{id}"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PluginActivationRecord {
    pub plugin_id: String,
    pub scope: PluginActivationScope,
    pub enabled: bool,
    pub revision: u64,
    pub updated_at_ms: u64,
    /// Observed install identity. It is a guard against a stale activation
    /// record reviving a different version or install location.
    #[serde(default)]
    pub installed_version: Option<String>,
    #[serde(default)]
    pub install_path: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ActivationFile {
    schema_version: u32,
    #[serde(default)]
    records: Vec<PluginActivationRecord>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PluginActivationSource {
    BuiltinPolicy,
    WorkspaceOverride,
    GlobalDefault,
    MigrationDefault,
    StaleRecord,
}

impl PluginActivationSource {
    pub fn label(&self) -> &'static str {
        match self {
            Self::BuiltinPolicy => "builtin policy",
            Self::WorkspaceOverride => "workspace override",
            Self::GlobalDefault => "global default",
            Self::MigrationDefault => "migration default",
            Self::StaleRecord => "stale activation record",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedPluginActivation {
    pub plugin_id: String,
    pub enabled: bool,
    pub source: PluginActivationSource,
    pub scope: PluginActivationScope,
    pub revision: u64,
    pub diagnostic: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedPluginActivationSet {
    workspace_id: Option<String>,
    revision: u64,
    entries: BTreeMap<String, ResolvedPluginActivation>,
    diagnostics: Vec<String>,
}

impl ResolvedPluginActivationSet {
    pub fn workspace_id(&self) -> Option<&str> {
        self.workspace_id.as_deref()
    }

    pub fn revision(&self) -> u64 {
        self.revision
    }

    pub fn entries(&self) -> impl Iterator<Item = &ResolvedPluginActivation> {
        self.entries.values()
    }

    pub fn get(&self, plugin_id: &str) -> Option<&ResolvedPluginActivation> {
        self.entries.get(plugin_id)
    }

    pub fn is_active(&self, plugin_id: &str) -> bool {
        self.entries
            .get(plugin_id)
            .is_some_and(|entry| entry.enabled)
    }

    pub fn active_plugin_ids(&self) -> HashSet<String> {
        self.entries
            .values()
            .filter(|entry| entry.enabled)
            .map(|entry| entry.plugin_id.clone())
            .collect()
    }

    pub fn diagnostics(&self) -> &[String] {
        &self.diagnostics
    }
}

#[derive(Debug, thiserror::Error)]
pub enum PluginActivationError {
    #[error("failed to read plugin activation state: {0}")]
    Read(std::io::Error),
    #[error("failed to write plugin activation state: {0}")]
    Write(std::io::Error),
    #[error("invalid plugin activation state: {0}")]
    Format(String),
}

/// Shared durable activation authority. The daemon singleton makes one
/// process the normal writer, while the mutex gives deterministic ordering
/// for concurrent management requests in that process.
#[derive(Clone)]
pub struct PluginActivationStore {
    path: Option<PathBuf>,
    state: Arc<Mutex<ActivationFile>>,
}

impl PluginActivationStore {
    pub async fn load(path: impl Into<PathBuf>) -> Result<Self, PluginActivationError> {
        let path = path.into();
        let file = match tokio::fs::read_to_string(&path).await {
            Ok(raw) => parse_file(&raw)?,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => ActivationFile {
                schema_version: ACTIVATION_SCHEMA_VERSION,
                records: Vec::new(),
            },
            Err(error) => return Err(PluginActivationError::Read(error)),
        };
        Ok(Self {
            path: Some(path),
            state: Arc::new(Mutex::new(file)),
        })
    }

    pub fn in_memory() -> Self {
        Self {
            path: None,
            state: Arc::new(Mutex::new(ActivationFile {
                schema_version: ACTIVATION_SCHEMA_VERSION,
                records: Vec::new(),
            })),
        }
    }

    pub async fn records(&self) -> Vec<PluginActivationRecord> {
        self.state.lock().await.records.clone()
    }

    pub async fn set(
        &self,
        plugin: &PluginInfo,
        scope: PluginActivationScope,
        enabled: bool,
    ) -> Result<PluginActivationRecord, PluginActivationError> {
        if let PluginActivationScope::Workspace(workspace_id) = &scope {
            codegg_core::identity::WorkspaceId::parse(workspace_id)
                .map_err(|error| PluginActivationError::Format(error.to_string()))?;
        }
        let mut state = self.state.lock().await;
        let old = state.clone();
        let revision = state
            .records
            .iter()
            .map(|record| record.revision)
            .max()
            .unwrap_or(0)
            .saturating_add(1);
        let record = PluginActivationRecord {
            plugin_id: plugin.id.clone(),
            scope,
            enabled,
            revision,
            updated_at_ms: now_ms(),
            installed_version: Some(plugin.manifest.version.clone()),
            install_path: install_path(plugin),
        };
        state.records.retain(|existing| {
            existing.plugin_id != record.plugin_id || existing.scope != record.scope
        });
        state.records.push(record.clone());
        if let Err(error) = self.persist(&state).await {
            *state = old;
            return Err(error);
        }
        Ok(record)
    }

    pub async fn remove_plugin(&self, plugin_id: &str) -> Result<(), PluginActivationError> {
        let mut state = self.state.lock().await;
        let old = state.clone();
        state.records.retain(|record| record.plugin_id != plugin_id);
        if state.records.len() != old.records.len() {
            if let Err(error) = self.persist(&state).await {
                *state = old;
                return Err(error);
            }
        }
        Ok(())
    }

    pub async fn resolve(
        &self,
        plugins: &[PluginInfo],
        workspace_id: Option<&str>,
    ) -> ResolvedPluginActivationSet {
        let state = self.state.lock().await.clone();
        resolve_records(&state.records, plugins, workspace_id)
    }

    async fn persist(&self, state: &ActivationFile) -> Result<(), PluginActivationError> {
        let Some(path) = &self.path else {
            return Ok(());
        };
        if let Some(parent) = path.parent() {
            tokio::fs::create_dir_all(parent)
                .await
                .map_err(PluginActivationError::Write)?;
        }
        let raw = serde_json::to_vec_pretty(state)
            .map_err(|error| PluginActivationError::Format(error.to_string()))?;
        let temp = path.with_extension(format!("json.tmp.{}", std::process::id()));
        tokio::fs::write(&temp, raw)
            .await
            .map_err(PluginActivationError::Write)?;
        if let Err(error) = tokio::fs::rename(&temp, path).await {
            let _ = tokio::fs::remove_file(&temp).await;
            return Err(PluginActivationError::Write(error));
        }
        Ok(())
    }
}

fn parse_file(raw: &str) -> Result<ActivationFile, PluginActivationError> {
    let file: ActivationFile = serde_json::from_str(raw)
        .map_err(|error| PluginActivationError::Format(error.to_string()))?;
    if file.schema_version != ACTIVATION_SCHEMA_VERSION {
        return Err(PluginActivationError::Format(format!(
            "unsupported schema version {}",
            file.schema_version
        )));
    }
    Ok(file)
}

fn resolve_records(
    records: &[PluginActivationRecord],
    plugins: &[PluginInfo],
    workspace_id: Option<&str>,
) -> ResolvedPluginActivationSet {
    let mut entries = BTreeMap::new();
    let mut diagnostics = Vec::new();
    let mut known = BTreeSet::new();
    let mut revision = 0;

    for plugin in plugins {
        known.insert(plugin.id.clone());
        let (enabled, source, scope, record_revision, diagnostic) = if is_builtin(plugin) {
            (
                true,
                PluginActivationSource::BuiltinPolicy,
                PluginActivationScope::Global,
                0,
                None,
            )
        } else {
            let workspace_record = workspace_id.and_then(|id| {
                records.iter().find(|record| {
                    record.plugin_id == plugin.id
                        && record.scope == PluginActivationScope::Workspace(id.to_string())
                })
            });
            let global_record = records.iter().find(|record| {
                record.plugin_id == plugin.id && record.scope == PluginActivationScope::Global
            });
            let selected = workspace_record.or(global_record);
            match selected {
                Some(record) if install_identity_matches(record, plugin) => (
                    record.enabled,
                    if matches!(record.scope, PluginActivationScope::Workspace(_)) {
                        PluginActivationSource::WorkspaceOverride
                    } else {
                        PluginActivationSource::GlobalDefault
                    },
                    record.scope.clone(),
                    record.revision,
                    None,
                ),
                Some(record) => {
                    let diagnostic = format!(
                        "plugin '{}' has a stale activation record at {}; remaining inactive",
                        plugin.id,
                        record.scope.label()
                    );
                    diagnostics.push(diagnostic.clone());
                    (
                        false,
                        PluginActivationSource::StaleRecord,
                        record.scope.clone(),
                        record.revision,
                        Some(diagnostic),
                    )
                }
                None => (
                    true,
                    PluginActivationSource::MigrationDefault,
                    PluginActivationScope::Global,
                    0,
                    None,
                ),
            }
        };
        revision = revision.max(record_revision);
        entries.insert(
            plugin.id.clone(),
            ResolvedPluginActivation {
                plugin_id: plugin.id.clone(),
                enabled,
                source,
                scope,
                revision: record_revision,
                diagnostic,
            },
        );
    }

    for record in records {
        if !known.contains(&record.plugin_id) {
            diagnostics.push(format!(
                "activation record for unknown plugin '{}' at {} is inactive",
                record.plugin_id,
                record.scope.label()
            ));
        }
        revision = revision.max(record.revision);
    }

    ResolvedPluginActivationSet {
        workspace_id: workspace_id.map(str::to_owned),
        revision,
        entries,
        diagnostics,
    }
}

fn is_builtin(plugin: &PluginInfo) -> bool {
    matches!(
        plugin.source.as_ref().map(|source| source.installed_by),
        Some(PluginInstallKind::Builtin)
    )
}

fn install_path(plugin: &PluginInfo) -> Option<String> {
    plugin
        .source
        .as_ref()
        .and_then(|source| source.install_path.as_ref())
        .map(|path| path.to_string_lossy().into_owned())
}

fn install_identity_matches(record: &PluginActivationRecord, plugin: &PluginInfo) -> bool {
    record.installed_version.as_deref() == Some(plugin.manifest.version.as_str())
        && record.install_path == install_path(plugin)
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plugin::manifest::PluginManifest;
    use crate::plugin::registry::PluginSourceMetadata;

    fn plugin(id: &str, version: &str, enabled: bool) -> PluginInfo {
        PluginInfo {
            id: id.into(),
            manifest: PluginManifest {
                name: id.into(),
                version: version.into(),
                ..Default::default()
            },
            enabled,
            trust: crate::plugin::manifest::PluginTrustClass::TrustedLocal,
            diagnostics: Vec::new(),
            source: Some(PluginSourceMetadata::registry_loaded(PathBuf::from(
                format!("/plugins/{id}"),
            ))),
        }
    }

    #[tokio::test(flavor = "current_thread")]
    async fn global_and_workspace_precedence_is_deterministic() {
        let store = PluginActivationStore::in_memory();
        let p = plugin("demo", "1", true);
        store
            .set(&p, PluginActivationScope::Global, false)
            .await
            .unwrap();
        store
            .set(&p, PluginActivationScope::Workspace("a".into()), true)
            .await
            .unwrap();

        let a = store.resolve(&[p.clone()], Some("a")).await;
        let b = store.resolve(&[p], Some("b")).await;
        assert!(a.is_active("demo"));
        assert!(!b.is_active("demo"));
        assert_eq!(
            a.get("demo").unwrap().source,
            PluginActivationSource::WorkspaceOverride
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn records_survive_store_reconstruction() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("activation.json");
        let store = PluginActivationStore::load(&path).await.unwrap();
        let p = plugin("demo", "1", true);
        store
            .set(&p, PluginActivationScope::Global, false)
            .await
            .unwrap();
        let restored = PluginActivationStore::load(&path).await.unwrap();
        let resolved = restored.resolve(&[p], None).await;
        assert!(!resolved.is_active("demo"));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn stale_identity_is_inactive_and_diagnosed() {
        let store = PluginActivationStore::in_memory();
        let p = plugin("demo", "1", true);
        store
            .set(&p, PluginActivationScope::Global, true)
            .await
            .unwrap();
        let changed = plugin("demo", "2", true);
        let resolved = store.resolve(&[changed], None).await;
        assert!(!resolved.is_active("demo"));
        assert!(resolved
            .diagnostics()
            .iter()
            .any(|diagnostic| diagnostic.contains("stale")));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn builtin_policy_is_not_disabled_by_third_party_records() {
        let store = PluginActivationStore::in_memory();
        let mut p = plugin("builtin", "1", true);
        p.trust = crate::plugin::manifest::PluginTrustClass::Builtin;
        p.source = Some(super::super::registry::PluginSourceMetadata::builtin());
        store
            .set(&p, PluginActivationScope::Global, false)
            .await
            .unwrap();
        assert!(store.resolve(&[p], None).await.is_active("builtin"));
    }
}

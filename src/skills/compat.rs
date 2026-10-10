use std::path::PathBuf;
use std::sync::Arc;

use crate::error::AppError;

use super::candidate::EffectiveSkill;
use super::registry::AssetRegistry;

#[derive(Debug, Clone)]
pub struct SkillIndexCompat {
    registry: Arc<AssetRegistry>,
}

impl Default for SkillIndexCompat {
    fn default() -> Self {
        Self::new()
    }
}

impl SkillIndexCompat {
    pub fn new() -> Self {
        Self {
            registry: Arc::new(AssetRegistry {
                effective: Vec::new(),
                diagnostics: Vec::new(),
                sources: Vec::new(),
            }),
        }
    }

    pub async fn load(&mut self, project_dir: &str) -> Result<(), AppError> {
        let global_roots: Vec<PathBuf> =
            crate::agent::asset_context::default_global_discovery_root()
                .into_iter()
                .collect();
        self.load_with_discovery_roots(project_dir, dirs::home_dir(), &global_roots)
            .await
    }

    /// Compatibility loader with an explicit user home for isolated callers
    /// and fixtures. `None` disables home-relative skill discovery.
    pub async fn load_with_home(
        &mut self,
        project_dir: &str,
        home_dir: Option<PathBuf>,
    ) -> Result<(), AppError> {
        let global_roots: Vec<PathBuf> =
            crate::agent::asset_context::default_global_discovery_root()
                .into_iter()
                .collect();
        self.load_with_discovery_roots(project_dir, home_dir, &global_roots)
            .await
    }

    /// Fully explicit root variant for deterministic compatibility tests and
    /// embedders with an existing discovery context.
    pub async fn load_with_discovery_roots(
        &mut self,
        project_dir: &str,
        home_dir: Option<PathBuf>,
        global_roots: &[PathBuf],
    ) -> Result<(), AppError> {
        let project_root = PathBuf::from(project_dir);
        let app_config = crate::config::schema::Config::load_or_default();
        let config = crate::agent::asset_context::asset_discovery_config_from(&app_config);
        let registry =
            AssetRegistry::build_with_home(&config, &project_root, global_roots, home_dir);
        self.registry = Arc::new(registry);
        Ok(())
    }

    pub fn get(&self, name: &str) -> Option<&EffectiveSkill> {
        self.registry.get(name)
    }

    pub fn list(&self) -> &[EffectiveSkill] {
        self.registry.list()
    }

    pub fn find_matching(&self, query: &str) -> Vec<&EffectiveSkill> {
        self.registry.find_matching(query)
    }

    pub fn build_system_prompt(&self) -> String {
        self.registry.build_system_prompt()
    }

    pub fn activate(&self, name: &str) -> Option<String> {
        self.registry.activate(name)
    }

    pub fn registry(&self) -> &Arc<AssetRegistry> {
        &self.registry
    }
}

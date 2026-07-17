use std::path::PathBuf;
use std::sync::Arc;

use crate::error::AppError;

use super::candidate::EffectiveSkill;
use super::registry::AssetRegistry;
use super::source::AssetDiscoveryConfig;

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
        let project_root = PathBuf::from(project_dir);

        let config_dir = dirs::config_dir()
            .map(|d| d.join("codegg").join("skills"))
            .filter(|d| d.is_dir());

        let global_roots: Vec<PathBuf> = config_dir
            .into_iter()
            .filter_map(|p| p.parent().map(|pp| pp.to_path_buf()))
            .collect();

        let config = AssetDiscoveryConfig::default();
        let registry = AssetRegistry::build(&config, &project_root, &global_roots);
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

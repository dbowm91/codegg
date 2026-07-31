//! Resolved, immutable tool authority for one agent turn.
//!
//! The resolver deliberately operates on model-facing definitions and small
//! metadata inputs.  It does not own permission prompts or execution; those
//! remain the canonical permission checker and broker.  Its job is to make
//! the advertised surface deterministic and to ensure that prompt/schema
//! construction has one auditable input.

use crate::provider::ToolDefinition;
use crate::tool::{ToolBackendKind, ToolCategory};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Capability {
    FilesystemRead,
    FilesystemWrite,
    ShellReadonly,
    ShellMutating,
    GitRead,
    GitWrite,
    NetworkResearch,
    Delegate,
    ManageTodos,
    ManageGoals,
    Terminal,
    Image,
}

/// Typed authority summary.  This is intentionally independent of agent
/// names and roles: labels may select prompts, never execution authority.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct AgentCapabilitySet {
    pub filesystem_read: bool,
    pub filesystem_write: bool,
    pub shell_readonly: bool,
    pub shell_mutating: bool,
    pub git_read: bool,
    pub git_write: bool,
    pub network_research: bool,
    pub delegate: bool,
    pub manage_todos: bool,
    pub manage_goals: bool,
    pub terminal: bool,
    pub image: bool,
}

impl AgentCapabilitySet {
    pub fn allows(self, capability: Capability) -> bool {
        match capability {
            Capability::FilesystemRead => self.filesystem_read,
            Capability::FilesystemWrite => self.filesystem_write,
            Capability::ShellReadonly => self.shell_readonly,
            Capability::ShellMutating => self.shell_mutating,
            Capability::GitRead => self.git_read,
            Capability::GitWrite => self.git_write,
            Capability::NetworkResearch => self.network_research,
            Capability::Delegate => self.delegate,
            Capability::ManageTodos => self.manage_todos,
            Capability::ManageGoals => self.manage_goals,
            Capability::Terminal => self.terminal,
            Capability::Image => self.image,
        }
    }

    pub fn intersect(self, ceiling: Self) -> Self {
        Self {
            filesystem_read: self.filesystem_read && ceiling.filesystem_read,
            filesystem_write: self.filesystem_write && ceiling.filesystem_write,
            shell_readonly: self.shell_readonly && ceiling.shell_readonly,
            shell_mutating: self.shell_mutating && ceiling.shell_mutating,
            git_read: self.git_read && ceiling.git_read,
            git_write: self.git_write && ceiling.git_write,
            network_research: self.network_research && ceiling.network_research,
            delegate: self.delegate && ceiling.delegate,
            manage_todos: self.manage_todos && ceiling.manage_todos,
            manage_goals: self.manage_goals && ceiling.manage_goals,
            terminal: self.terminal && ceiling.terminal,
            image: self.image && ceiling.image,
        }
    }

    pub fn capabilities(self) -> BTreeSet<Capability> {
        [
            Capability::FilesystemRead,
            Capability::FilesystemWrite,
            Capability::ShellReadonly,
            Capability::ShellMutating,
            Capability::GitRead,
            Capability::GitWrite,
            Capability::NetworkResearch,
            Capability::Delegate,
            Capability::ManageTodos,
            Capability::ManageGoals,
            Capability::Terminal,
            Capability::Image,
        ]
        .into_iter()
        .filter(|c| self.allows(*c))
        .collect()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ToolOmissionReason {
    Denied,
    PlanMode,
    DisabledByModel,
    MissingBackend,
    NonCallable,
    ParentCeiling,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolOmission {
    pub canonical_name: String,
    pub reason: ToolOmissionReason,
}

#[derive(Debug, Clone)]
pub struct ResolvedTool {
    pub canonical_name: String,
    pub wire_name: String,
    pub backend: ToolBackendKind,
    pub category: ToolCategory,
    pub definition: ToolDefinition,
    pub required: bool,
    pub never_reduce: bool,
}

#[derive(Debug, Clone)]
pub struct ResolvedToolSurface {
    pub tools: Vec<ResolvedTool>,
    pub canonical_to_wire: BTreeMap<String, String>,
    pub wire_to_canonical: BTreeMap<String, String>,
    pub capabilities: AgentCapabilitySet,
    pub fingerprint: String,
    pub omissions: Vec<ToolOmission>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SurfaceError {
    AliasCollision(String),
    AmbiguousReverseAlias(String),
    InvalidDefinition(String),
}

impl ResolvedToolSurface {
    /// Resolve the native registry surface used by prompt compilation.
    /// MCP definitions are added by `AgentLoop` before provider dispatch.
    pub fn from_registry(
        registry: &crate::tool::ToolRegistry,
        denied: &BTreeSet<String>,
        disabled: &BTreeSet<String>,
        plan_mode: bool,
        parent_ceiling: Option<AgentCapabilitySet>,
    ) -> Result<Self, SurfaceError> {
        Self::from_registry_with_aliases(
            registry,
            denied,
            disabled,
            plan_mode,
            parent_ceiling,
            &BTreeMap::new(),
        )
    }

    pub fn from_registry_with_aliases(
        registry: &crate::tool::ToolRegistry,
        denied: &BTreeSet<String>,
        disabled: &BTreeSet<String>,
        plan_mode: bool,
        parent_ceiling: Option<AgentCapabilitySet>,
        wire_to_canonical_aliases: &BTreeMap<String, String>,
    ) -> Result<Self, SurfaceError> {
        let definitions = registry
            .list()
            .into_iter()
            .filter(|tool| tool.expose_in_definitions())
            .map(|tool| ToolDefinition {
                name: tool.name().to_string(),
                description: tool.description().to_string(),
                parameters: tool.parameters(),
                defer_loading: tool.defer_loading().then_some(true),
            })
            .collect::<Vec<_>>();
        let has_functional_spawner = registry
            .list()
            .into_iter()
            .find(|tool| tool.name() == "task")
            .is_some_and(|tool| tool.has_functional_backend());
        Self::resolve_with_aliases(
            definitions,
            denied,
            disabled,
            plan_mode,
            has_functional_spawner,
            parent_ceiling,
            wire_to_canonical_aliases,
        )
    }

    /// Resolve a surface from already policy-filtered definitions.  Native
    /// definitions are callable by construction; `task` additionally needs
    /// the explicit functional-spawner bit.  MCP names are namespaced and
    /// therefore cannot shadow native tools.
    pub fn resolve(
        definitions: impl IntoIterator<Item = ToolDefinition>,
        denied: &BTreeSet<String>,
        disabled: &BTreeSet<String>,
        plan_mode: bool,
        has_functional_spawner: bool,
        parent_ceiling: Option<AgentCapabilitySet>,
    ) -> Result<Self, SurfaceError> {
        Self::resolve_with_aliases(
            definitions,
            denied,
            disabled,
            plan_mode,
            has_functional_spawner,
            parent_ceiling,
            &BTreeMap::new(),
        )
    }

    /// Resolve with provider/model wire names mapped to stable canonical
    /// names.  Aliases are applied before omission and capability checks.
    pub fn resolve_with_aliases(
        definitions: impl IntoIterator<Item = ToolDefinition>,
        denied: &BTreeSet<String>,
        disabled: &BTreeSet<String>,
        plan_mode: bool,
        has_functional_spawner: bool,
        parent_ceiling: Option<AgentCapabilitySet>,
        wire_to_canonical_aliases: &BTreeMap<String, String>,
    ) -> Result<Self, SurfaceError> {
        let mut tools = Vec::new();
        let mut omissions = Vec::new();
        let mut canonical_to_wire = BTreeMap::new();
        let mut wire_to_canonical = BTreeMap::new();
        let mut capabilities = AgentCapabilitySet::default();

        let mut definitions: Vec<_> = definitions.into_iter().collect();
        definitions.sort_by(|a, b| a.name.cmp(&b.name));
        for definition in definitions {
            if definition.name.is_empty() {
                return Err(SurfaceError::InvalidDefinition("empty tool name".into()));
            }
            let wire_name = definition.name.clone();
            let canonical_name = wire_to_canonical_aliases
                .get(&wire_name)
                .cloned()
                .unwrap_or_else(|| canonical_name(&wire_name));
            let category = category_for_name(&canonical_name);
            let backend = if canonical_name.starts_with("mcp__") {
                ToolBackendKind::Mcp
            } else if matches!(category, ToolCategory::ShellExec) {
                ToolBackendKind::Shell
            } else {
                ToolBackendKind::Native
            };

            let reason = if denied.contains(&canonical_name) || denied.contains(&wire_name) {
                Some(ToolOmissionReason::Denied)
            } else if disabled.contains(&canonical_name) || disabled.contains(&wire_name) {
                Some(ToolOmissionReason::DisabledByModel)
            } else if plan_mode && !plan_allowed(&canonical_name) {
                Some(ToolOmissionReason::PlanMode)
            } else if canonical_name == "task" && !has_functional_spawner {
                Some(ToolOmissionReason::NonCallable)
            } else {
                None
            };
            if let Some(reason) = reason {
                omissions.push(ToolOmission {
                    canonical_name,
                    reason,
                });
                continue;
            }

            if let Some(existing) = canonical_to_wire.get(&canonical_name) {
                if existing != &wire_name {
                    return Err(SurfaceError::AliasCollision(canonical_name));
                }
            }
            if let Some(existing) = wire_to_canonical.get(&wire_name) {
                if existing != &canonical_name {
                    return Err(SurfaceError::AmbiguousReverseAlias(wire_name));
                }
            }
            canonical_to_wire.insert(canonical_name.clone(), wire_name.clone());
            wire_to_canonical.insert(wire_name.clone(), canonical_name.clone());
            add_capabilities(&mut capabilities, &canonical_name, category);
            tools.push(ResolvedTool {
                required: matches!(canonical_name.as_str(), "read" | "tool_search"),
                never_reduce: matches!(canonical_name.as_str(), "read" | "tool_search"),
                canonical_name,
                wire_name,
                backend,
                category,
                definition,
            });
        }

        if let Some(ceiling) = parent_ceiling {
            capabilities = capabilities.intersect(ceiling);
            tools.retain(|tool| {
                let allowed = tool_capabilities(&tool.canonical_name, tool.category)
                    .into_iter()
                    .all(|cap| capabilities.allows(cap));
                if !allowed {
                    omissions.push(ToolOmission {
                        canonical_name: tool.canonical_name.clone(),
                        reason: ToolOmissionReason::ParentCeiling,
                    });
                }
                allowed
            });
            canonical_to_wire.retain(|name, _| tools.iter().any(|t| &t.canonical_name == name));
            wire_to_canonical.retain(|_, name| tools.iter().any(|t| &t.canonical_name == name));
        }

        let fingerprint = fingerprint(&tools, &canonical_to_wire, &capabilities);
        Ok(Self {
            tools,
            canonical_to_wire,
            wire_to_canonical,
            capabilities,
            fingerprint,
            omissions,
        })
    }

    pub fn definitions(&self) -> Vec<ToolDefinition> {
        self.tools
            .iter()
            .map(|tool| tool.definition.clone())
            .collect()
    }

    pub fn reduce(&self, max: usize) -> Vec<ToolDefinition> {
        if self.tools.len() <= max {
            return self.definitions();
        }
        let mut selected: Vec<_> = self
            .tools
            .iter()
            .filter(|t| t.required || t.never_reduce)
            .collect();
        selected.extend(
            self.tools
                .iter()
                .filter(|t| !t.required && !t.never_reduce)
                .take(max.saturating_sub(selected.len())),
        );
        selected.into_iter().map(|t| t.definition.clone()).collect()
    }

    pub fn canonical_name_for_wire(&self, wire_name: &str) -> Option<&str> {
        self.wire_to_canonical.get(wire_name).map(String::as_str)
    }
}

fn canonical_name(name: &str) -> String {
    name.to_string()
}

fn category_for_name(name: &str) -> ToolCategory {
    crate::permission::tool_category_for_name(name)
}

fn plan_allowed(name: &str) -> bool {
    matches!(
        name,
        "read"
            | "glob"
            | "grep"
            | "list"
            | "codesearch"
            | "webfetch"
            | "lsp"
            | "skill"
            | "todoread"
            | "todowrite"
            | "bash"
            | "plan_enter"
            | "plan_exit"
            | "tool_search"
    )
}

fn tool_capabilities(name: &str, category: ToolCategory) -> Vec<Capability> {
    let mut result = Vec::new();
    if name == "task" {
        // Delegation is its own authority.  The task tool does not grant the
        // parent filesystem mutation merely because its implementation is
        // conservatively categorized as mutating.
        result.push(Capability::Delegate);
        return result;
    }
    match category {
        ToolCategory::ReadOnly => result.push(Capability::FilesystemRead),
        ToolCategory::SafeMutating => result.push(Capability::ManageTodos),
        ToolCategory::Mutating => result.push(Capability::FilesystemWrite),
        ToolCategory::ShellExec => {
            result.push(Capability::ShellReadonly);
            result.push(Capability::ShellMutating);
        }
    }
    if name == "terminal" {
        result.push(Capability::Terminal);
    }
    if matches!(
        name,
        "websearch" | "webfetch" | "research" | "research_search"
    ) {
        result.push(Capability::NetworkResearch);
    }
    if name == "image" {
        result.push(Capability::Image);
    }
    if name == "git" {
        result.push(Capability::GitRead);
        result.push(Capability::GitWrite);
    }
    result
}

fn add_capabilities(set: &mut AgentCapabilitySet, name: &str, category: ToolCategory) {
    for cap in tool_capabilities(name, category) {
        match cap {
            Capability::FilesystemRead => set.filesystem_read = true,
            Capability::FilesystemWrite => set.filesystem_write = true,
            Capability::ShellReadonly => set.shell_readonly = true,
            Capability::ShellMutating => set.shell_mutating = true,
            Capability::GitRead => set.git_read = true,
            Capability::GitWrite => set.git_write = true,
            Capability::NetworkResearch => set.network_research = true,
            Capability::Delegate => set.delegate = true,
            Capability::ManageTodos => set.manage_todos = true,
            Capability::ManageGoals => set.manage_goals = true,
            Capability::Terminal => set.terminal = true,
            Capability::Image => set.image = true,
        }
    }
}

fn fingerprint(
    tools: &[ResolvedTool],
    aliases: &BTreeMap<String, String>,
    capabilities: &AgentCapabilitySet,
) -> String {
    let mut material = String::new();
    for tool in tools {
        material.push_str(&tool.canonical_name);
        material.push('\0');
        material.push_str(&tool.wire_name);
        material.push('\0');
        material.push_str(&serde_json::to_string(&tool.definition.parameters).unwrap_or_default());
        material.push('\n');
    }
    for (canonical, wire) in aliases {
        material.push_str(canonical);
        material.push('=');
        material.push_str(wire);
        material.push('\n');
    }
    material.push_str(&format!("caps:{:?}", capabilities.capabilities()));
    format!("sha256:{:x}", Sha256::digest(material.as_bytes()))
}

#[cfg(test)]
mod tests {
    use super::*;
    fn def(name: &str) -> ToolDefinition {
        ToolDefinition {
            name: name.into(),
            description: name.into(),
            parameters: serde_json::json!({"type":"object"}),
            defer_loading: None,
        }
    }

    #[test]
    fn task_requires_functional_spawner() {
        let surface = ResolvedToolSurface::resolve(
            [def("read"), def("task")],
            &BTreeSet::new(),
            &BTreeSet::new(),
            false,
            false,
            None,
        )
        .unwrap();
        assert_eq!(
            surface
                .tools
                .iter()
                .map(|t| t.canonical_name.as_str())
                .collect::<Vec<_>>(),
            vec!["read"]
        );
        assert_eq!(surface.omissions[0].reason, ToolOmissionReason::NonCallable);
    }

    #[test]
    fn child_ceiling_cannot_widen() {
        let ceiling = AgentCapabilitySet {
            filesystem_read: true,
            ..Default::default()
        };
        let surface = ResolvedToolSurface::resolve(
            [def("read"), def("write")],
            &BTreeSet::new(),
            &BTreeSet::new(),
            false,
            false,
            Some(ceiling),
        )
        .unwrap();
        assert_eq!(surface.tools.len(), 1);
        assert_eq!(surface.tools[0].canonical_name, "read");
    }

    #[test]
    fn fingerprint_is_order_independent() {
        let a = ResolvedToolSurface::resolve(
            [def("read"), def("write")],
            &BTreeSet::new(),
            &BTreeSet::new(),
            false,
            false,
            None,
        )
        .unwrap();
        let b = ResolvedToolSurface::resolve(
            [def("write"), def("read")],
            &BTreeSet::new(),
            &BTreeSet::new(),
            false,
            false,
            None,
        )
        .unwrap();
        assert_eq!(a.fingerprint, b.fingerprint);
    }
}

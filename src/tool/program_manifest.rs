//! Manifest resolution for Tool Programs.
//!
//! Validates that a program's requested tools are available,
//! properly contracted, and permitted before job creation.
//! This is the gate between program submission and execution.

use std::collections::HashSet;

use crate::tool::broker::ToolBroker;
use crate::tool::contract::{ToolCallerPolicy, ToolContract};

/// Result of manifest resolution.
#[derive(Debug, Clone)]
pub struct ResolvedManifest {
    /// Tools that were resolved and are callable.
    pub allowed_tools: Vec<ToolContract>,
    /// Tools that were requested but rejected.
    pub rejected: Vec<ManifestRejection>,
}

/// Why a tool was rejected from the manifest.
#[derive(Debug, Clone)]
pub struct ManifestRejection {
    pub tool_name: String,
    pub reason: RejectionReason,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RejectionReason {
    /// Tool not found in the broker catalog.
    NotFound,
    /// Tool is DirectOnly, not callable by programs.
    DirectOnly,
    /// Tool has no output schema defined.
    NoOutputSchema,
    /// Tool contract validation failed.
    InvalidContract(String),
}

impl std::fmt::Display for RejectionReason {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotFound => write!(f, "tool not found in catalog"),
            Self::DirectOnly => write!(f, "tool is direct-only, not callable by programs"),
            Self::NoOutputSchema => write!(f, "tool has no output schema"),
            Self::InvalidContract(msg) => write!(f, "invalid contract: {}", msg),
        }
    }
}

/// Resolve a set of requested tool names against the broker catalog.
///
/// Returns a `ResolvedManifest` with allowed and rejected tools.
/// Programs must only use tools in the `allowed_tools` list.
pub fn resolve_manifest(broker: &ToolBroker, requested_tools: &[String]) -> ResolvedManifest {
    let mut allowed = Vec::new();
    let mut rejected = Vec::new();
    let mut seen = HashSet::new();

    for tool_name in requested_tools {
        if !seen.insert(tool_name) {
            continue; // deduplicate
        }

        let contract = match broker.lookup_contract(tool_name) {
            Ok(c) => c.clone(),
            Err(_) => {
                rejected.push(ManifestRejection {
                    tool_name: tool_name.clone(),
                    reason: RejectionReason::NotFound,
                });
                continue;
            }
        };

        // Must be callable by programs
        if contract.caller_policy == ToolCallerPolicy::DirectOnly {
            rejected.push(ManifestRejection {
                tool_name: tool_name.clone(),
                reason: RejectionReason::DirectOnly,
            });
            continue;
        }

        // Must have an output schema
        if contract.output_schema.is_none() {
            rejected.push(ManifestRejection {
                tool_name: tool_name.clone(),
                reason: RejectionReason::NoOutputSchema,
            });
            continue;
        }

        // Validate contract consistency
        if let Err(e) = contract.validate() {
            rejected.push(ManifestRejection {
                tool_name: tool_name.clone(),
                reason: RejectionReason::InvalidContract(e.to_string()),
            });
            continue;
        }

        allowed.push(contract);
    }

    ResolvedManifest {
        allowed_tools: allowed,
        rejected,
    }
}

/// Check if a manifest resolution is fully valid (no rejections).
pub fn manifest_is_valid(resolved: &ResolvedManifest) -> bool {
    resolved.rejected.is_empty()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tool::broker::ToolBroker;
    use crate::tool::contract::{ToolCallerPolicy, ToolContract, ToolEffectClass};
    use crate::tool::{Tool, ToolCategory, ToolRegistry};
    use async_trait::async_trait;

    struct MockReadOnlyTool {
        name: String,
    }

    #[async_trait]
    impl Tool for MockReadOnlyTool {
        fn name(&self) -> &str {
            &self.name
        }
        fn description(&self) -> &str {
            "mock read-only tool"
        }
        fn parameters(&self) -> serde_json::Value {
            serde_json::json!({"type": "object"})
        }
        async fn execute(
            &self,
            _input: serde_json::Value,
        ) -> Result<String, crate::error::ToolError> {
            Ok("mock output".to_string())
        }
        fn category(&self) -> ToolCategory {
            ToolCategory::ReadOnly
        }
        fn contract(&self, tool_name: &str, input_schema: serde_json::Value) -> ToolContract {
            ToolContract {
                name: tool_name.to_string(),
                caller_policy: ToolCallerPolicy::DirectOrProgrammatic,
                effect_class: ToolEffectClass::ReadOnly,
                output_schema: Some(serde_json::json!({"type": "object"})),
                ..ToolContract::legacy(tool_name, input_schema)
            }
        }
    }

    struct MockDirectOnlyTool;

    #[async_trait]
    impl Tool for MockDirectOnlyTool {
        fn name(&self) -> &str {
            "direct_only"
        }
        fn description(&self) -> &str {
            "mock direct-only tool"
        }
        fn parameters(&self) -> serde_json::Value {
            serde_json::json!({"type": "object"})
        }
        async fn execute(
            &self,
            _input: serde_json::Value,
        ) -> Result<String, crate::error::ToolError> {
            Ok("direct only".to_string())
        }
        fn category(&self) -> ToolCategory {
            ToolCategory::ReadOnly
        }
        // No contract override — defaults to DirectOnly
    }

    struct MockNoSchemaTool;

    #[async_trait]
    impl Tool for MockNoSchemaTool {
        fn name(&self) -> &str {
            "no_schema"
        }
        fn description(&self) -> &str {
            "tool without schema"
        }
        fn parameters(&self) -> serde_json::Value {
            serde_json::json!({"type": "object"})
        }
        async fn execute(
            &self,
            _input: serde_json::Value,
        ) -> Result<String, crate::error::ToolError> {
            Ok("no schema".to_string())
        }
        fn category(&self) -> ToolCategory {
            ToolCategory::ReadOnly
        }
        fn contract(&self, tool_name: &str, input_schema: serde_json::Value) -> ToolContract {
            ToolContract {
                name: tool_name.to_string(),
                caller_policy: ToolCallerPolicy::DirectOrProgrammatic,
                effect_class: ToolEffectClass::ReadOnly,
                output_schema: None, // No schema!
                ..ToolContract::legacy(tool_name, input_schema)
            }
        }
    }

    fn make_broker() -> ToolBroker {
        let mut registry = ToolRegistry::with_defaults();
        registry.register(MockReadOnlyTool {
            name: "readable".to_string(),
        });
        registry.register(MockReadOnlyTool {
            name: "searchable".to_string(),
        });
        registry.register(MockDirectOnlyTool);
        registry.register(MockNoSchemaTool);
        ToolBroker::new(&registry)
    }

    #[test]
    fn resolve_manifest_allows_valid_tools() {
        let broker = make_broker();
        let resolved = resolve_manifest(&broker, &["readable".into(), "searchable".into()]);
        assert!(resolved.rejected.is_empty());
        assert_eq!(resolved.allowed_tools.len(), 2);
        assert!(manifest_is_valid(&resolved));
    }

    #[test]
    fn resolve_manifest_rejects_direct_only() {
        let broker = make_broker();
        let resolved = resolve_manifest(&broker, &["direct_only".into()]);
        assert_eq!(resolved.rejected.len(), 1);
        assert_eq!(resolved.rejected[0].reason, RejectionReason::DirectOnly);
        assert!(!manifest_is_valid(&resolved));
    }

    #[test]
    fn resolve_manifest_rejects_no_schema() {
        let broker = make_broker();
        let resolved = resolve_manifest(&broker, &["no_schema".into()]);
        assert_eq!(resolved.rejected.len(), 1);
        assert_eq!(resolved.rejected[0].reason, RejectionReason::NoOutputSchema);
    }

    #[test]
    fn resolve_manifest_rejects_unknown() {
        let broker = make_broker();
        let resolved = resolve_manifest(&broker, &["nonexistent".into()]);
        assert_eq!(resolved.rejected.len(), 1);
        assert_eq!(resolved.rejected[0].reason, RejectionReason::NotFound);
    }

    #[test]
    fn resolve_manifest_deduplicates() {
        let broker = make_broker();
        let resolved = resolve_manifest(&broker, &["readable".into(), "readable".into()]);
        assert_eq!(resolved.allowed_tools.len(), 1);
    }

    #[test]
    fn resolve_manifest_mixed_valid_and_invalid() {
        let broker = make_broker();
        let resolved = resolve_manifest(
            &broker,
            &[
                "readable".into(),
                "direct_only".into(),
                "nonexistent".into(),
            ],
        );
        assert_eq!(resolved.allowed_tools.len(), 1);
        assert_eq!(resolved.rejected.len(), 2);
    }
}

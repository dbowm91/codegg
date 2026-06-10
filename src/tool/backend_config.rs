//! Conversion from config-time schema types to runtime/tool types.
//!
//! The configuration loader parses TOML/JSON into
//! `crate::config::schema::ToolBackendConfigSchema` and its
//! sub-types. Tool wrappers and the registry consume the
//! runtime-side equivalents in `crate::tool::backend`. This module
//! provides the single explicit bridge so the registry and wrappers
//! never have to consult config-time types directly.
//!
//! See `plans/native_tool_crates_hardening.md` Phase 2 for context.

use crate::config::schema::{
    ExternalToolBackendConfigSchema, ToolBackendConfigSchema, ToolImplementationBackendSchema,
};
use crate::tool::backend::{
    ExternalToolBackendConfig, ToolBackendConfig, ToolImplementationBackend,
};

impl From<ToolImplementationBackendSchema> for ToolImplementationBackend {
    fn from(s: ToolImplementationBackendSchema) -> Self {
        match s {
            ToolImplementationBackendSchema::Native => ToolImplementationBackend::Native,
            ToolImplementationBackendSchema::Mcp => ToolImplementationBackend::Mcp,
            ToolImplementationBackendSchema::Builtin => ToolImplementationBackend::Builtin,
            ToolImplementationBackendSchema::Disabled => ToolImplementationBackend::Disabled,
        }
    }
}

impl From<&ToolImplementationBackendSchema> for ToolImplementationBackend {
    fn from(s: &ToolImplementationBackendSchema) -> Self {
        ToolImplementationBackend::from(*s)
    }
}

impl From<&ExternalToolBackendConfigSchema> for ExternalToolBackendConfig {
    fn from(s: &ExternalToolBackendConfigSchema) -> Self {
        Self {
            backend: s.backend.map(ToolImplementationBackend::from),
            expose_raw_mcp_tools: s.expose_raw_mcp_tools,
            fallback_to_native: s.fallback_to_native,
            server_name: s.server_name.clone(),
            command: s.command.clone(),
            args: s.args.clone(),
            timeout_ms: s.timeout_ms,
            env: s.env.clone(),
        }
    }
}

impl From<&ToolBackendConfigSchema> for ToolBackendConfig {
    fn from(s: &ToolBackendConfigSchema) -> Self {
        Self {
            lsp: s.lsp.as_ref().map(ExternalToolBackendConfig::from),
            security: s.security.as_ref().map(ExternalToolBackendConfig::from),
            context: s.context.as_ref().map(ExternalToolBackendConfig::from),
        }
    }
}

impl ToolBackendConfig {
    /// Build a `ToolBackendConfig` from a loaded `Config`. When the
    /// config has no `[tool_backends]` section, returns the all-native
    /// default (so domains without explicit configuration are
    /// authoritative native).
    pub fn from_config(config: &crate::config::schema::Config) -> Self {
        config
            .tool_backends
            .as_ref()
            .map(ToolBackendConfig::from)
            .unwrap_or_else(ToolBackendConfig::all_native)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::schema::Config;

    #[test]
    fn from_schema_native_roundtrip() {
        let s = ToolBackendConfigSchema {
            lsp: Some(ExternalToolBackendConfigSchema {
                backend: Some(ToolImplementationBackendSchema::Native),
                ..Default::default()
            }),
            security: Some(ExternalToolBackendConfigSchema {
                backend: Some(ToolImplementationBackendSchema::Native),
                ..Default::default()
            }),
            context: None,
        };
        let cfg = ToolBackendConfig::from(&s);
        assert_eq!(
            cfg.backend_for(crate::tool::backend::BackendDomain::Lsp),
            ToolImplementationBackend::Native
        );
        assert_eq!(
            cfg.backend_for(crate::tool::backend::BackendDomain::Security),
            ToolImplementationBackend::Native
        );
        assert_eq!(
            cfg.backend_for(crate::tool::backend::BackendDomain::Context),
            ToolImplementationBackend::Native
        );
    }

    #[test]
    fn from_schema_mcp_propagates() {
        let s = ToolBackendConfigSchema {
            lsp: Some(ExternalToolBackendConfigSchema {
                backend: Some(ToolImplementationBackendSchema::Mcp),
                expose_raw_mcp_tools: Some(true),
                fallback_to_native: Some(false),
                server_name: Some("egglsp".to_string()),
                ..Default::default()
            }),
            ..Default::default()
        };
        let cfg = ToolBackendConfig::from(&s);
        assert_eq!(
            cfg.backend_for(crate::tool::backend::BackendDomain::Lsp),
            ToolImplementationBackend::Mcp
        );
        let lsp = cfg.lsp.expect("lsp section");
        assert!(lsp.expose_raw_mcp_tools());
        assert!(!lsp.fallback_to_native());
        assert_eq!(lsp.server_name.as_deref(), Some("egglsp"));
    }

    #[test]
    fn from_schema_builtin_propagates() {
        let s = ToolBackendConfigSchema {
            security: Some(ExternalToolBackendConfigSchema {
                backend: Some(ToolImplementationBackendSchema::Builtin),
                ..Default::default()
            }),
            ..Default::default()
        };
        let cfg = ToolBackendConfig::from(&s);
        assert_eq!(
            cfg.backend_for(crate::tool::backend::BackendDomain::Security),
            ToolImplementationBackend::Builtin
        );
    }

    #[test]
    fn from_schema_disabled_propagates() {
        let s = ToolBackendConfigSchema {
            lsp: Some(ExternalToolBackendConfigSchema {
                backend: Some(ToolImplementationBackendSchema::Disabled),
                ..Default::default()
            }),
            ..Default::default()
        };
        let cfg = ToolBackendConfig::from(&s);
        assert_eq!(
            cfg.backend_for(crate::tool::backend::BackendDomain::Lsp),
            ToolImplementationBackend::Disabled
        );
    }

    #[test]
    fn from_config_no_section_returns_all_native() {
        let cfg = Config::default();
        let runtime = ToolBackendConfig::from_config(&cfg);
        assert_eq!(
            runtime.backend_for(crate::tool::backend::BackendDomain::Lsp),
            ToolImplementationBackend::Native
        );
        assert_eq!(
            runtime.backend_for(crate::tool::backend::BackendDomain::Security),
            ToolImplementationBackend::Native
        );
        assert_eq!(
            runtime.backend_for(crate::tool::backend::BackendDomain::Context),
            ToolImplementationBackend::Native
        );
    }

    #[test]
    fn from_config_with_section_uses_runtime_values() {
        let cfg = Config {
            tool_backends: Some(ToolBackendConfigSchema {
                security: Some(ExternalToolBackendConfigSchema {
                    backend: Some(ToolImplementationBackendSchema::Mcp),
                    ..Default::default()
                }),
                ..Default::default()
            }),
            ..Default::default()
        };
        let runtime = ToolBackendConfig::from_config(&cfg);
        assert_eq!(
            runtime.backend_for(crate::tool::backend::BackendDomain::Security),
            ToolImplementationBackend::Mcp
        );
    }
}

//! Snapshot-style tests that lock down the model-facing tool surface
//! of the default `ToolRegistry`.
//!
//! These tests are intentionally tolerant of unrelated additions
//! (new tools, new optional fields on existing tools) by
//! asserting names and categories, not full JSON schemas. That
//! keeps them from breaking on every cosmetic change while still
//! catching accidental contract drift.
//!
//! See `plans/native_tool_crates_hardening.md` Phase 11.

use codegg::tool::backend::{
    ExternalToolBackendConfig, ToolBackendConfig, ToolImplementationBackend,
};
use codegg::tool::{ToolCategory, ToolRegistry};

/// Tools that must always be present in a default registry.
const REQUIRED_TOOLS: &[&str] = &[
    "bash",
    "read",
    "edit",
    "write",
    "grep",
    "glob",
    "list",
    "websearch",
    "webfetch",
    "lsp",
    "security",
    "git",
    "commit",
    "review",
    "tool_search",
    "invalid",
    "question",
    "skill",
];

/// Tools that must NOT be present when their backend is disabled.
const DISABLABLE_TOOLS: &[&str] = &["lsp", "security"];

#[test]
fn default_registry_includes_required_native_tools() {
    let registry = ToolRegistry::with_defaults();
    let names: std::collections::BTreeSet<&str> =
        registry.list().iter().map(|t| t.name()).collect();
    for required in REQUIRED_TOOLS {
        assert!(
            names.contains(required),
            "default registry missing required tool `{required}`; got: {names:?}"
        );
    }
}

#[test]
fn default_tool_categories_are_consistent() {
    let registry = ToolRegistry::with_defaults();
    let read_only_expected = [
        "read", "grep", "glob", "list", "webfetch", "lsp", "security",
    ];
    for name in read_only_expected {
        let tool = registry
            .get(name)
            .unwrap_or_else(|| panic!("missing tool {name}"));
        assert_eq!(
            tool.category(),
            ToolCategory::ReadOnly,
            "expected {name} to be ReadOnly"
        );
    }
    let mutating_expected = ["edit", "write", "git", "commit"];
    for name in mutating_expected {
        let tool = registry
            .get(name)
            .unwrap_or_else(|| panic!("missing tool {name}"));
        assert_eq!(
            tool.category(),
            ToolCategory::Mutating,
            "expected {name} to be Mutating"
        );
    }
    assert_eq!(
        registry.get("bash").unwrap().category(),
        ToolCategory::ShellExec,
        "bash must be ShellExec"
    );
}

#[test]
fn disabled_lsp_backend_keeps_stub_but_hides_from_definitions() {
    let mut backends = ToolBackendConfig::all_native();
    backends.lsp = Some(ExternalToolBackendConfig {
        backend: Some(ToolImplementationBackend::Disabled),
        ..Default::default()
    });
    let registry = ToolRegistry::with_options(codegg::tool::ToolRegistryOptions {
        tool_backends: backends,
        ..Default::default()
    });
    // A `DisabledTool` stub is still registered for diagnostics.
    assert!(
        registry.contains("lsp"),
        "disabled lsp stub should remain in the registry for diagnostics"
    );
    let lsp = registry.get("lsp").unwrap();
    assert_eq!(lsp.name(), "lsp");
    // But the stub is hidden from the model-facing tool
    // definitions: the model never sees a tool whose every call
    // is a guaranteed failure.
    let defs = registry.definitions();
    assert!(
        !defs.iter().any(|d| d.name == "lsp"),
        "disabled lsp must not appear in model-facing definitions"
    );
}

#[test]
fn disabled_security_backend_keeps_stub_but_hides_from_definitions() {
    let mut backends = ToolBackendConfig::all_native();
    backends.security = Some(ExternalToolBackendConfig {
        backend: Some(ToolImplementationBackend::Disabled),
        ..Default::default()
    });
    let registry = ToolRegistry::with_options(codegg::tool::ToolRegistryOptions {
        tool_backends: backends,
        ..Default::default()
    });
    assert!(registry.contains("security"));
    let defs = registry.definitions();
    assert!(
        !defs.iter().any(|d| d.name == "security"),
        "disabled security must not appear in model-facing definitions"
    );
}

#[test]
fn mcp_lsp_with_fallback_keeps_lsp_in_definitions() {
    let mut backends = ToolBackendConfig::all_native();
    backends.lsp = Some(ExternalToolBackendConfig {
        backend: Some(ToolImplementationBackend::Mcp),
        server_name: Some("egglsp".to_string()),
        fallback_to_native: Some(true),
        ..Default::default()
    });
    let registry = ToolRegistry::with_options(codegg::tool::ToolRegistryOptions {
        tool_backends: backends,
        ..Default::default()
    });
    // The native LspTool is registered and exposed to the model
    // because fallback_to_native = true.
    let defs = registry.definitions();
    assert!(
        defs.iter().any(|d| d.name == "lsp"),
        "lsp must remain model-visible when MCP fallback is on"
    );
}

#[test]
fn mcp_lsp_without_fallback_hides_lsp_from_definitions() {
    let mut backends = ToolBackendConfig::all_native();
    backends.lsp = Some(ExternalToolBackendConfig {
        backend: Some(ToolImplementationBackend::Mcp),
        server_name: Some("egglsp".to_string()),
        fallback_to_native: Some(false),
        ..Default::default()
    });
    let registry = ToolRegistry::with_options(codegg::tool::ToolRegistryOptions {
        tool_backends: backends,
        ..Default::default()
    });
    let defs = registry.definitions();
    assert!(
        !defs.iter().any(|d| d.name == "lsp"),
        "lsp must NOT be model-visible when MCP fallback is off"
    );
}

#[test]
fn with_session_config_defaults_preserves_resolved_backends() {
    use codegg::config::schema::{
        ExternalToolBackendConfigSchema, ToolBackendConfigSchema, ToolImplementationBackendSchema,
    };
    let config = codegg::config::schema::Config {
        tool_backends: Some(ToolBackendConfigSchema {
            lsp: Some(ExternalToolBackendConfigSchema {
                backend: Some(ToolImplementationBackendSchema::Disabled),
                ..Default::default()
            }),
            ..Default::default()
        }),
        ..Default::default()
    };
    let todo_state =
        std::sync::Arc::new(tokio::sync::Mutex::new(codegg::task_state::TodoState::new()));
    let registry = ToolRegistry::with_session_config_defaults(
        &config,
        todo_state,
        codegg::model_profile::types::TaskStatePolicy::explicit_todo(),
        None,
        None,
    );
    // The resolved backend config flows through, so lsp is hidden
    // from the model.
    let defs = registry.definitions();
    assert!(
        !defs.iter().any(|d| d.name == "lsp"),
        "with_session_config_defaults must respect the loaded [tool_backends] config"
    );
    assert_eq!(
        registry
            .tool_backends()
            .backend_for(codegg::tool::backend::BackendDomain::Lsp),
        ToolImplementationBackend::Disabled
    );
}

#[tokio::test]
async fn with_session_config_defaults_wires_command_intent_to_bash() {
    use codegg::config::schema::{CommandIntentMode, RouteLevel};
    use serde_json::json;

    let mut config = codegg::config::schema::Config::default();
    config.command_intent = Some(codegg::config::schema::CommandIntentConfig {
        mode: Some(CommandIntentMode::Active),
        route_safe_commands: Some(true),
        route_tests: Some(RouteLevel::Active),
        ..Default::default()
    });
    let todo_state =
        std::sync::Arc::new(tokio::sync::Mutex::new(codegg::task_state::TodoState::new()));
    let registry = ToolRegistry::with_session_config_defaults(
        &config,
        todo_state,
        codegg::model_profile::types::TaskStatePolicy::explicit_todo(),
        None,
        None,
    );

    let output = registry
        .get("bash")
        .expect("bash must be registered")
        .execute(json!({"command": "cargo test --help"}))
        .await
        .expect("active routing should execute the supervised test path");
    assert!(
        output.starts_with("Test run passed."),
        "configured active routing must reach TestRunner, got: {output}"
    );
}

#[test]
fn disabling_lsp_does_not_remove_other_tools() {
    let mut backends = ToolBackendConfig::all_native();
    backends.lsp = Some(ExternalToolBackendConfig {
        backend: Some(ToolImplementationBackend::Disabled),
        ..Default::default()
    });
    let registry = ToolRegistry::with_options(codegg::tool::ToolRegistryOptions {
        tool_backends: backends,
        ..Default::default()
    });
    for name in REQUIRED_TOOLS {
        if DISABLABLE_TOOLS.contains(name) {
            continue;
        }
        assert!(
            registry.contains(name),
            "expected `{name}` to remain registered when only lsp is disabled"
        );
    }
}

#[test]
fn websearch_and_webfetch_use_native_names() {
    let registry = ToolRegistry::with_defaults();
    let ws = registry.get("websearch").expect("websearch registered");
    let wf = registry.get("webfetch").expect("webfetch registered");
    assert_eq!(ws.name(), "websearch");
    assert_eq!(wf.name(), "webfetch");
    assert_eq!(ws.category(), ToolCategory::ReadOnly);
    assert_eq!(wf.category(), ToolCategory::ReadOnly);
}

#[test]
fn backend_report_includes_three_configurable_domains() {
    let registry = ToolRegistry::with_defaults();
    let report = registry.backend_report(None);
    assert_eq!(report.len(), 3);
    let domains: std::collections::BTreeSet<&str> = report.iter().map(|r| r.domain).collect();
    assert!(domains.contains("lsp"));
    assert!(domains.contains("security"));
    assert!(domains.contains("context"));
}

#[test]
fn with_config_populates_resolved_backends() {
    use codegg::config::schema::{
        ExternalToolBackendConfigSchema, ToolBackendConfigSchema, ToolImplementationBackendSchema,
    };
    let config = codegg::config::schema::Config {
        tool_backends: Some(ToolBackendConfigSchema {
            lsp: Some(ExternalToolBackendConfigSchema {
                backend: Some(ToolImplementationBackendSchema::Disabled),
                ..Default::default()
            }),
            ..Default::default()
        }),
        ..Default::default()
    };
    let registry = ToolRegistry::with_config(&config);
    let lsp = registry
        .tool_backends()
        .backend_for(codegg::tool::backend::BackendDomain::Lsp);
    assert_eq!(lsp, ToolImplementationBackend::Disabled);
}

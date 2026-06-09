//! Smoke tests for the structured execution path
//! (`ToolRegistry::execute_capture`) and the model-facing
//! definition visibility of disabled / fallback tools.
//!
//! These tests intentionally do not require network, LSP servers,
//! or running MCP servers. They are pure in-process tests that
//! lock down the contract documented in
//! `plans/native_tool_runtime_correctness.md` Phase 6.
//!
//! See also: `architecture/native_crates.md` and
//! `architecture/tool.md`.

use codegg::tool::backend::{
    ExternalToolBackendConfig, ToolBackendConfig, ToolBackendKind, ToolImplementationBackend,
    ToolProvenance, ToolTrust,
};
use codegg::tool::{ToolRegistry, ToolRegistryOptions};
use serde_json::json;

fn build_with_backends(backends: ToolBackendConfig) -> ToolRegistry {
    ToolRegistry::with_options(ToolRegistryOptions {
        tool_backends: backends,
        ..ToolRegistryOptions::default()
    })
}

/// `list` is a legacy tool that does not override
/// `execute_structured`. `execute_capture` should still succeed and
/// return a `StructuredToolResult` with legacy provenance.
#[tokio::test]
async fn list_through_execute_capture_returns_legacy_provenance() {
    let registry = ToolRegistry::with_defaults();
    let result = registry
        .execute_capture("list", json!({"path": ".", "max_depth": 0}), None)
        .await
        .expect("list should succeed");
    assert!(result.success, "list should report success");
    // Legacy provenance is attached for tools that don't
    // override `execute_structured`.
    let p = result
        .provenance
        .as_ref()
        .expect("execute_capture attaches legacy provenance");
    assert_eq!(p.implementation, "list");
    assert_eq!(p.backend, "builtinlegacy");
    assert_eq!(p.trust, ToolTrust::LocalUntrusted);
    assert!(
        p.elapsed_ms.is_some(),
        "execute_capture stamps elapsed_ms even for legacy tools"
    );
}

/// `security` overrides `execute_structured` to attach
/// `eggsentry` provenance when the native security wrapper is the
/// live path.
#[tokio::test]
async fn security_through_execute_capture_reports_eggsentry_provenance() {
    let registry = ToolRegistry::with_defaults();
    let result = registry
        .execute_capture(
            "security",
            json!({"action": "classify_command", "command": "ls"}),
            None,
        )
        .await
        .expect("security should succeed");
    let p = result
        .provenance
        .as_ref()
        .expect("security attaches structured provenance");
    assert_eq!(
        p.implementation, "eggsentry",
        "native security tool reports its real implementation"
    );
    assert_eq!(p.backend, "native");
}

/// Disabled `security` is registered as a stub but is *not*
/// model-visible. A direct call to `execute_capture` on the stub
/// still surfaces the configured reason.
#[tokio::test]
async fn disabled_security_is_not_model_visible_but_callable() {
    let mut backends = ToolBackendConfig::all_native();
    backends.security = Some(ExternalToolBackendConfig {
        backend: Some(ToolImplementationBackend::Disabled),
        ..Default::default()
    });
    let registry = build_with_backends(backends);

    // Hidden from model definitions.
    let defs = registry.definitions();
    assert!(
        !defs.iter().any(|d| d.name == "security"),
        "disabled security must not appear in model-facing definitions"
    );

    // Calling the stub still surfaces the configured reason.
    let err = registry
        .execute_capture(
            "security",
            json!({"action": "classify_command", "command": "ls"}),
            None,
        )
        .await
        .expect_err("disabled security stub should always error");
    assert!(
        err.to_string()
            .contains("security backend is configured as 'disabled'"),
        "got: {err}"
    );
}

/// `Mcp + fallback_to_native = true` registers the native
/// wrapper, which appears in `definitions()` and is the live path.
#[tokio::test]
async fn mcp_fallback_true_registers_native_security() {
    let mut backends = ToolBackendConfig::all_native();
    backends.security = Some(ExternalToolBackendConfig {
        backend: Some(ToolImplementationBackend::Mcp),
        server_name: Some("eggsentry".to_string()),
        fallback_to_native: Some(true),
        ..Default::default()
    });
    let registry = build_with_backends(backends);

    let defs = registry.definitions();
    assert!(
        defs.iter().any(|d| d.name == "security"),
        "native security should be model-visible when fallback is on"
    );

    let result = registry
        .execute_capture(
            "security",
            json!({"action": "classify_command", "command": "ls"}),
            None,
        )
        .await
        .expect("native security should succeed");
    let p = result
        .provenance
        .as_ref()
        .expect("native security attaches structured provenance");
    assert_eq!(p.implementation, "eggsentry");
    assert_eq!(p.backend, "native");
}

/// `Mcp + fallback_to_native = false` does not expose the tool to
/// the model. A stub is registered for diagnostics.
#[tokio::test]
async fn mcp_fallback_false_hides_security_from_model() {
    let mut backends = ToolBackendConfig::all_native();
    backends.security = Some(ExternalToolBackendConfig {
        backend: Some(ToolImplementationBackend::Mcp),
        server_name: Some("eggsentry".to_string()),
        fallback_to_native: Some(false),
        ..Default::default()
    });
    let registry = build_with_backends(backends);

    let defs = registry.definitions();
    assert!(
        !defs.iter().any(|d| d.name == "security"),
        "security must not be model-visible when MCP fallback is off"
    );

    let report = registry.backend_report(None);
    let sec = report.iter().find(|r| r.domain == "security").unwrap();
    assert_eq!(
        sec.status,
        codegg::tool::RegistryBackendStatusKind::ConfiguredButUnavailable
    );
}

/// `ToolRegistry::execute_capture` returns
/// `ToolError::NotFound` for unknown tools instead of panicking.
#[tokio::test]
async fn execute_capture_unknown_tool_returns_not_found() {
    let registry = ToolRegistry::with_defaults();
    let err = registry
        .execute_capture("definitely_not_a_real_tool", json!({}), None)
        .await
        .expect_err("unknown tool should fail");
    match err {
        codegg::error::ToolError::NotFound(name) => {
            assert_eq!(name, "definitely_not_a_real_tool");
        }
        other => panic!("expected NotFound, got {other:?}"),
    }
}

/// `execute_capture` stamps `elapsed_ms` on legacy provenance.
#[tokio::test]
async fn execute_capture_legacy_records_elapsed_ms() {
    let registry = ToolRegistry::with_defaults();
    let result = registry
        .execute_capture("list", json!({"path": ".", "max_depth": 0}), None)
        .await
        .expect("list should succeed");
    let p = result
        .provenance
        .as_ref()
        .expect("legacy provenance present");
    assert!(
        p.elapsed_ms.unwrap_or(u64::MAX) < 30_000,
        "elapsed_ms should be a small non-negative value, got {:?}",
        p.elapsed_ms
    );
}

/// All model-facing domain backends that are configured-but-
/// unavailable report `unavailable` consistently.
#[test]
fn mcp_no_fallback_consistent_across_lsp_and_security() {
    let mut backends = ToolBackendConfig::all_native();
    backends.lsp = Some(ExternalToolBackendConfig {
        backend: Some(ToolImplementationBackend::Mcp),
        server_name: Some("egglsp".to_string()),
        fallback_to_native: Some(false),
        ..Default::default()
    });
    backends.security = Some(ExternalToolBackendConfig {
        backend: Some(ToolImplementationBackend::Mcp),
        server_name: Some("eggsentry".to_string()),
        fallback_to_native: Some(false),
        ..Default::default()
    });
    let registry = build_with_backends(backends);
    let report = registry.backend_report(Some(&[]));
    for row in &report {
        if row.domain == "lsp" || row.domain == "security" {
            assert_eq!(
                row.status,
                codegg::tool::RegistryBackendStatusKind::ConfiguredButUnavailable,
                "{} should be unavailable, got {:?}",
                row.domain,
                row.status
            );
            assert_eq!(row.backend, "mcp");
        }
    }
    // And neither should be model-visible.
    let defs = registry.definitions();
    assert!(!defs.iter().any(|d| d.name == "lsp"));
    assert!(!defs.iter().any(|d| d.name == "security"));
}

/// Helper trait surface check: `ToolProvenance::legacy` and
/// `StructuredToolResult::legacy` are public and stable.
#[test]
fn provenance_helpers_are_public_and_stable() {
    let p = ToolProvenance::legacy("foo");
    assert_eq!(p.implementation, "foo");
    assert_eq!(p.backend, "builtinlegacy");
    assert_eq!(p.trust, ToolTrust::LocalUntrusted);

    // `ToolBackendKind` carries a `label()` accessor used by the
    // structured path. Lock the contract down so refactors
    // notice.
    assert_eq!(ToolBackendKind::Native.label(), "Native");
}

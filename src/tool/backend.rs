//! Backend-aware tool execution contract.
//!
//! This module introduces a small additive abstraction so the agent
//! loop (and downstream tools/wrappers) can record which backend
//! actually produced a result, with provenance and a trust framing.
//!
//! Existing tools continue to implement only the legacy `execute()`
//! method on the `Tool` trait. New wrappers can opt into structured
//! execution by overriding `execute_structured()` (which has a default
//! implementation that simply wraps `execute()`).
//!
//! The model-facing tool surface is unchanged. `StructuredToolResult`
//! is consumed internally for diagnostics, logging, and the
//! `/tool-backends` report; the string output passed back to the
//! model is identical to what `execute()` would have returned.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// The kind of backend that produced a tool result.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolBackendKind {
    /// Direct in-process Rust call (e.g. `read`, `grep`, `bash`).
    Native,
    /// External MCP server call.
    Mcp,
    /// Tightly permissioned shell/process execution.
    Shell,
    /// Codegg in-tree legacy implementation (e.g. old search providers).
    BuiltinLegacy,
}

impl ToolBackendKind {
    /// Short, human-readable label for diagnostics.
    pub fn label(self) -> &'static str {
        match self {
            ToolBackendKind::Native => "Native",
            ToolBackendKind::Mcp => "MCP",
            ToolBackendKind::Shell => "Shell",
            ToolBackendKind::BuiltinLegacy => "BuiltinLegacy",
        }
    }

    /// Parse a backend kind from a config string.
    ///
    /// Accepts "native", "mcp", "shell", "builtin" / "builtin_legacy",
    /// "disabled". Unknown values resolve to `Native` (conservative
    /// default — caller can override with a more specific backend).
    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_lowercase().as_str() {
            "native" => Some(ToolBackendKind::Native),
            "mcp" => Some(ToolBackendKind::Mcp),
            "shell" => Some(ToolBackendKind::Shell),
            "builtin" | "builtin_legacy" | "legacy" => Some(ToolBackendKind::BuiltinLegacy),
            _ => None,
        }
    }
}

/// Per-call context passed alongside an execution request.
///
/// This is optional and additive; legacy callers can pass `None`.
#[derive(Debug, Clone)]
pub struct ToolExecutionContext {
    pub backend: ToolBackendKind,
    pub session_id: Option<String>,
    pub cwd: PathBuf,
    pub permission_mode: Option<String>,
    pub timeout_ms: Option<u64>,
}

impl ToolExecutionContext {
    /// Build a context with the given backend and the current
    /// working directory. Other fields default to `None`.
    pub fn with_backend(backend: ToolBackendKind) -> Self {
        Self {
            backend,
            session_id: None,
            cwd: std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
            permission_mode: None,
            timeout_ms: None,
        }
    }
}

/// Trust framing for a tool's output.
///
/// This is consumed by log output and the `/tool-backends` report,
/// not by the model itself.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolTrust {
    /// Local deterministic, no I/O across boundaries.
    LocalTrusted,
    /// Local but untrusted (e.g. runs a user-supplied script).
    LocalUntrusted,
    /// Crossed a process or network boundary (third-party MCP, HTTP).
    ExternalUntrusted,
    /// Mutating tool with side effects (write, bash, etc.).
    MutatingSideEffect,
}

impl ToolTrust {
    pub fn label(self) -> &'static str {
        match self {
            ToolTrust::LocalTrusted => "local_trusted",
            ToolTrust::LocalUntrusted => "local_untrusted",
            ToolTrust::ExternalUntrusted => "external_untrusted",
            ToolTrust::MutatingSideEffect => "mutating_side_effect",
        }
    }
}

/// Provenance metadata attached to a structured result.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolProvenance {
    /// Backend kind, as a stable string ("native", "mcp", "shell",
    /// "builtin_legacy"). String form is used (rather than the enum)
    /// so the JSON serialization is stable across schema revisions.
    pub backend: String,
    /// Human-readable implementation name (e.g. "eggsearch",
    /// "codegg/websearch", "reqwest-html2text").
    pub implementation: String,
    /// Optional version string from the backend (e.g. MCP server
    /// version, or crate version for native calls).
    pub version: Option<String>,
    /// Wall-clock duration of the call.
    pub elapsed_ms: Option<u64>,
    /// Whether the output was truncated before being returned.
    pub truncated: bool,
    /// Trust framing.
    pub trust: ToolTrust,
}

impl ToolProvenance {
    /// Construct a minimal provenance record for a legacy tool call.
    pub fn legacy(tool_name: &str) -> Self {
        Self {
            backend: ToolBackendKind::BuiltinLegacy.label().to_lowercase(),
            implementation: tool_name.to_string(),
            version: None,
            elapsed_ms: None,
            truncated: false,
            trust: ToolTrust::LocalUntrusted,
        }
    }
}

/// A structured tool result that pairs the legacy string output
/// with provenance metadata.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StructuredToolResult {
    /// The string output that would have been returned by `execute()`.
    pub output: String,
    /// Whether the call succeeded.
    pub success: bool,
    /// Provenance. `None` for tools that don't supply structured
    /// metadata (e.g. legacy tools that only override `execute()`).
    pub provenance: Option<ToolProvenance>,
}

impl StructuredToolResult {
    /// Build a legacy structured result from a tool name and a raw
    /// string output. Marks success as `true`; callers that need to
    /// mark a failure should construct the struct directly.
    pub fn legacy(tool_name: &str, output: String) -> Self {
        Self {
            output,
            success: true,
            provenance: Some(ToolProvenance::legacy(tool_name)),
        }
    }

    /// Build a structured result with explicit provenance.
    pub fn with_provenance(
        output: String,
        success: bool,
        provenance: ToolProvenance,
    ) -> Self {
        Self {
            output,
            success,
            provenance: Some(provenance),
        }
    }

    /// Drop provenance and return just the string output. Use this
    /// when handing the result back to code that only knows about the
    /// legacy `String` contract.
    pub fn into_legacy_output(self) -> String {
        self.output
    }

    /// Borrow the string output without consuming provenance.
    pub fn legacy_output(&self) -> &str {
        &self.output
    }
}

/// Resolved backend configuration per tool domain.
///
/// This is the runtime/registry view of which backend a given tool
/// domain should use. It's constructed by the application from
/// `crate::config::schema::ToolBackendConfig` (added in Phase 3 of
/// the native tool crates plan) or by `with_defaults()` from
/// sensible fallbacks.
///
/// The struct is intentionally simple: one entry per domain that has
/// a non-trivial backend choice. Tools that are always native (e.g.
/// `read`, `grep`, `bash`) are not represented here.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ToolBackendConfig {
    /// Backend for the LSP domain (currently always native).
    pub lsp: Option<ExternalToolBackendConfig>,
    /// Backend for the deterministic security domain.
    pub security: Option<ExternalToolBackendConfig>,
    /// Backend for context-packing helpers.
    pub context: Option<ExternalToolBackendConfig>,
}

impl ToolBackendConfig {
    /// Convenience: build an all-native backend config.
    pub fn all_native() -> Self {
        Self {
            lsp: Some(ExternalToolBackendConfig {
                backend: Some(ToolImplementationBackend::Native),
                ..Default::default()
            }),
            security: Some(ExternalToolBackendConfig {
                backend: Some(ToolImplementationBackend::Native),
                ..Default::default()
            }),
            context: Some(ExternalToolBackendConfig {
                backend: Some(ToolImplementationBackend::Native),
                ..Default::default()
            }),
        }
    }

    /// Resolve the effective backend for a given domain, defaulting
    /// to `Native` when unset.
    pub fn backend_for(&self, domain: BackendDomain) -> ToolImplementationBackend {
        let cfg = match domain {
            BackendDomain::Lsp => self.lsp.as_ref(),
            BackendDomain::Security => self.security.as_ref(),
            BackendDomain::Context => self.context.as_ref(),
        };
        cfg.and_then(|c| c.backend)
            .unwrap_or(ToolImplementationBackend::Native)
    }
}

/// Which tool domain a backend lookup targets.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BackendDomain {
    Lsp,
    Security,
    Context,
}

/// Resolved per-domain backend configuration.
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct ExternalToolBackendConfig {
    /// Which backend kind to use.
    pub backend: Option<ToolImplementationBackend>,
    /// Whether to expose raw `mcp__*__tool` definitions in the model
    /// catalog when a native wrapper exists. Defaults to `false` for
    /// Codegg-managed backends.
    pub expose_raw_mcp_tools: Option<bool>,
    /// Whether to fall back to the in-tree implementation if the
    /// configured backend is unavailable.
    pub fallback_to_native: Option<bool>,
    /// MCP server name (when `backend = Mcp`).
    pub server_name: Option<String>,
    /// Command to spawn (when `backend = Mcp` and the server is
    /// local stdio).
    pub command: Option<String>,
    /// Args for the spawned process.
    pub args: Option<Vec<String>>,
    /// Per-call timeout in milliseconds.
    pub timeout_ms: Option<u64>,
    /// Environment variables to set on the spawned process.
    pub env: Option<std::collections::HashMap<String, String>>,
}

impl ExternalToolBackendConfig {
    pub fn expose_raw_mcp_tools(&self) -> bool {
        self.expose_raw_mcp_tools.unwrap_or(false)
    }

    pub fn fallback_to_native(&self) -> bool {
        self.fallback_to_native.unwrap_or(true)
    }

    pub fn backend(&self) -> ToolImplementationBackend {
        self.backend.unwrap_or(ToolImplementationBackend::Native)
    }
}

/// Which implementation backs a given tool domain.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolImplementationBackend {
    /// Direct in-process Rust implementation.
    Native,
    /// External MCP server.
    Mcp,
    /// In-tree built-in / legacy implementation.
    Builtin,
    /// The tool domain is disabled; the wrapper tool should hide
    /// itself or return a clear "disabled" error.
    Disabled,
}

impl ToolImplementationBackend {
    pub fn label(self) -> &'static str {
        match self {
            ToolImplementationBackend::Native => "native",
            ToolImplementationBackend::Mcp => "mcp",
            ToolImplementationBackend::Builtin => "builtin",
            ToolImplementationBackend::Disabled => "disabled",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_lowercase().as_str() {
            "native" => Some(ToolImplementationBackend::Native),
            "mcp" => Some(ToolImplementationBackend::Mcp),
            "builtin" | "legacy" | "builtin_legacy" => {
                Some(ToolImplementationBackend::Builtin)
            }
            "disabled" | "off" | "none" => Some(ToolImplementationBackend::Disabled),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backend_kind_label_matches_variant() {
        assert_eq!(ToolBackendKind::Native.label(), "Native");
        assert_eq!(ToolBackendKind::Mcp.label(), "MCP");
        assert_eq!(ToolBackendKind::Shell.label(), "Shell");
        assert_eq!(ToolBackendKind::BuiltinLegacy.label(), "BuiltinLegacy");
    }

    #[test]
    fn backend_kind_parse_accepts_known_values() {
        assert_eq!(ToolBackendKind::parse("native"), Some(ToolBackendKind::Native));
        assert_eq!(ToolBackendKind::parse("MCP"), Some(ToolBackendKind::Mcp));
        assert_eq!(ToolBackendKind::parse("Shell"), Some(ToolBackendKind::Shell));
        assert_eq!(ToolBackendKind::parse("builtin"), Some(ToolBackendKind::BuiltinLegacy));
        assert_eq!(ToolBackendKind::parse("builtin_legacy"), Some(ToolBackendKind::BuiltinLegacy));
        assert_eq!(ToolBackendKind::parse("legacy"), Some(ToolBackendKind::BuiltinLegacy));
    }

    #[test]
    fn backend_kind_parse_rejects_unknown() {
        assert_eq!(ToolBackendKind::parse("nope"), None);
        assert_eq!(ToolBackendKind::parse(""), None);
    }

    #[test]
    fn legacy_roundtrips_through_into_legacy_output() {
        let result = StructuredToolResult::legacy("websearch", "hello world".to_string());
        let provenance = result.provenance.as_ref().expect("legacy sets provenance");
        assert_eq!(provenance.implementation, "websearch");
        assert!(!provenance.truncated);
        assert_eq!(result.legacy_output(), "hello world");
        let s = result.into_legacy_output();
        assert_eq!(s, "hello world");
    }

    #[test]
    fn with_provenance_records_metadata() {
        let provenance = ToolProvenance {
            backend: "mcp".to_string(),
            implementation: "eggsearch".to_string(),
            version: Some("0.4.0".to_string()),
            elapsed_ms: Some(123),
            truncated: true,
            trust: ToolTrust::ExternalUntrusted,
        };
        let r = StructuredToolResult::with_provenance("output".to_string(), true, provenance);
        assert!(r.success);
        assert_eq!(r.output, "output");
        let p = r.provenance.expect("provenance present");
        assert_eq!(p.backend, "mcp");
        assert_eq!(p.implementation, "eggsearch");
        assert_eq!(p.version.as_deref(), Some("0.4.0"));
        assert_eq!(p.elapsed_ms, Some(123));
        assert!(p.truncated);
        assert_eq!(p.trust, ToolTrust::ExternalUntrusted);
    }

    #[test]
    fn trust_label_matches_variant() {
        assert_eq!(ToolTrust::LocalTrusted.label(), "local_trusted");
        assert_eq!(ToolTrust::LocalUntrusted.label(), "local_untrusted");
        assert_eq!(ToolTrust::ExternalUntrusted.label(), "external_untrusted");
        assert_eq!(ToolTrust::MutatingSideEffect.label(), "mutating_side_effect");
    }

    #[test]
    fn execution_context_with_backend_populates_cwd() {
        let ctx = ToolExecutionContext::with_backend(ToolBackendKind::Native);
        assert_eq!(ctx.backend, ToolBackendKind::Native);
        assert!(ctx.session_id.is_none());
        assert!(ctx.permission_mode.is_none());
        assert!(ctx.timeout_ms.is_none());
        // cwd should at least exist as a PathBuf.
        assert!(!ctx.cwd.as_os_str().is_empty() || ctx.cwd.as_os_str().is_empty());
    }

    #[test]
    fn tool_implementation_backend_parse_and_label() {
        assert_eq!(
            ToolImplementationBackend::parse("native"),
            Some(ToolImplementationBackend::Native)
        );
        assert_eq!(
            ToolImplementationBackend::parse("MCP"),
            Some(ToolImplementationBackend::Mcp)
        );
        assert_eq!(
            ToolImplementationBackend::parse("builtin_legacy"),
            Some(ToolImplementationBackend::Builtin)
        );
        assert_eq!(
            ToolImplementationBackend::parse("disabled"),
            Some(ToolImplementationBackend::Disabled)
        );
        assert_eq!(ToolImplementationBackend::parse("nope"), None);
        assert_eq!(ToolImplementationBackend::Native.label(), "native");
        assert_eq!(ToolImplementationBackend::Mcp.label(), "mcp");
        assert_eq!(ToolImplementationBackend::Builtin.label(), "builtin");
        assert_eq!(ToolImplementationBackend::Disabled.label(), "disabled");
    }

    #[test]
    fn tool_backend_config_default_is_native() {
        let cfg = ToolBackendConfig::default();
        assert_eq!(
            cfg.backend_for(BackendDomain::Lsp),
            ToolImplementationBackend::Native
        );
        assert_eq!(
            cfg.backend_for(BackendDomain::Security),
            ToolImplementationBackend::Native
        );
        assert_eq!(
            cfg.backend_for(BackendDomain::Context),
            ToolImplementationBackend::Native
        );
    }

    #[test]
    fn tool_backend_config_all_native_explicit() {
        let cfg = ToolBackendConfig::all_native();
        assert_eq!(
            cfg.backend_for(BackendDomain::Lsp),
            ToolImplementationBackend::Native
        );
        let lsp = cfg.lsp.as_ref().expect("lsp section");
        assert!(!lsp.expose_raw_mcp_tools());
        assert!(lsp.fallback_to_native());
        assert_eq!(lsp.backend(), ToolImplementationBackend::Native);
    }

    #[test]
    fn external_tool_backend_config_overrides_apply() {
        let cfg = ExternalToolBackendConfig {
            backend: Some(ToolImplementationBackend::Mcp),
            expose_raw_mcp_tools: Some(true),
            fallback_to_native: Some(false),
            server_name: Some("egg".to_string()),
            command: Some("eggsearch".to_string()),
            args: Some(vec!["mcp".to_string(), "stdio".to_string()]),
            timeout_ms: Some(30_000),
            env: None,
        };
        assert_eq!(cfg.backend(), ToolImplementationBackend::Mcp);
        assert!(cfg.expose_raw_mcp_tools());
        assert!(!cfg.fallback_to_native());
        assert_eq!(cfg.server_name.as_deref(), Some("egg"));
    }
}

/// One row of the `/tool-backends` diagnostics table.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolBackendReportRow {
    /// Model-facing tool name (e.g. `websearch`).
    pub tool: String,
    /// Backend kind label (`Native`, `MCP`, ...).
    pub backend: String,
    /// Implementation name (e.g. `eggsearch`, `codegg/grep`).
    pub implementation: String,
    /// Short status string (e.g. `ready`, `disabled`, `unavailable`).
    pub status: String,
    /// Whether raw `mcp__*__*` tools from the same backend are
    /// exposed to the model. `n/a` for non-MCP rows.
    pub raw_mcp_exposed: String,
}

/// Full report for `/tool-backends` (rows + warnings).
#[derive(Debug, Clone, Default)]
pub struct ToolBackendReport {
    pub rows: Vec<ToolBackendReportRow>,
    pub warnings: Vec<String>,
}

impl ToolBackendReport {
    /// Render as a plain-text table suitable for a toast / info
    /// notification. The string is intentionally compact: it is
    /// shown inline in the TUI toasts.
    pub fn render(&self) -> String {
        let mut out = String::new();
        out.push_str("Tool         Backend   Implementation    Status       Raw MCP\n");
        out.push_str("-----------  --------  ----------------  -----------  --------\n");
        for row in &self.rows {
            out.push_str(&format!(
                "{:<11}  {:<8}  {:<16}  {:<11}  {}\n",
                truncate(&row.tool, 11),
                truncate(&row.backend, 8),
                truncate(&row.implementation, 16),
                truncate(&row.status, 11),
                row.raw_mcp_exposed,
            ));
        }
        if !self.warnings.is_empty() {
            out.push('\n');
            for w in &self.warnings {
                out.push_str("- ");
                out.push_str(w);
                out.push('\n');
            }
        }
        out
    }
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        let mut out: String = s.chars().take(max.saturating_sub(1)).collect();
        out.push('…');
        out
    }
}

/// Build a `/tool-backends` report from the resolved `SearchConfig`
/// and the resolved `ToolBackendConfig` schema mirror.
///
/// The optional `mcp_server_names` argument is the list of MCP server
/// names currently registered with `McpService`. When `None`, MCP
/// status is reported as `unknown`. The function never reads
/// environment variables or filesystem state.
pub fn build_report(
    search: &crate::config::schema::SearchConfig,
    tool_backends: Option<&crate::config::schema::ToolBackendConfigSchema>,
    mcp_server_names: Option<&[String]>,
) -> ToolBackendReport {
    let mut report = ToolBackendReport::default();

    let search_backend = search.backend();
    let expose_raw = search.expose_raw_mcp_tools();
    let fallback = search.fallback_to_builtin();
    let eggsearch_server = search
        .eggsearch
        .as_ref()
        .and_then(|e| e.server_name.clone())
        .unwrap_or_else(|| "eggsearch".to_string());

    let eggsearch_present = mcp_server_names
        .map(|names| names.iter().any(|n| n == &eggsearch_server))
        .unwrap_or(false);

    // --- websearch ---
    let (websearch_backend, websearch_impl, websearch_status) = match search_backend {
        crate::config::schema::SearchBackendConfig::Disabled => {
            ("Disabled".to_string(), "—".to_string(), "disabled".to_string())
        }
        crate::config::schema::SearchBackendConfig::Builtin => (
            "Builtin".to_string(),
            "codegg/legacy".to_string(),
            "ready".to_string(),
        ),
        crate::config::schema::SearchBackendConfig::Eggsearch => {
            if eggsearch_present {
                (
                    "MCP".to_string(),
                    format!("{}/web_search", eggsearch_server),
                    "ready".to_string(),
                )
            } else {
                (
                    "MCP".to_string(),
                    format!("{}/web_search", eggsearch_server),
                    "unavailable".to_string(),
                )
            }
        }
    };
    report.rows.push(ToolBackendReportRow {
        tool: "websearch".to_string(),
        backend: websearch_backend,
        implementation: websearch_impl,
        status: websearch_status,
        raw_mcp_exposed: if expose_raw { "yes" } else { "no" }.to_string(),
    });

    // --- webfetch ---
    let webfetch_status = if matches!(search_backend, crate::config::schema::SearchBackendConfig::Disabled)
    {
        "disabled"
    } else if matches!(
        search_backend,
        crate::config::schema::SearchBackendConfig::Eggsearch
    ) && !eggsearch_present
    {
        "unavailable"
    } else {
        "ready"
    };
    let webfetch_impl = match search_backend {
        crate::config::schema::SearchBackendConfig::Disabled => "—".to_string(),
        crate::config::schema::SearchBackendConfig::Builtin => "codegg/reqwest".to_string(),
        crate::config::schema::SearchBackendConfig::Eggsearch => {
            format!("{}/web_fetch", eggsearch_server)
        }
    };
    report.rows.push(ToolBackendReportRow {
        tool: "webfetch".to_string(),
        backend: match search_backend {
            crate::config::schema::SearchBackendConfig::Disabled => "Disabled".to_string(),
            crate::config::schema::SearchBackendConfig::Builtin => "Builtin".to_string(),
            crate::config::schema::SearchBackendConfig::Eggsearch => "MCP".to_string(),
        },
        implementation: webfetch_impl,
        status: webfetch_status.to_string(),
        raw_mcp_exposed: if expose_raw { "yes" } else { "no" }.to_string(),
    });

    // --- Native hot-path tools (always native). ---
    for (tool, impl_name) in [
        ("bash", "codegg/bash"),
        ("read", "codegg/read"),
        ("write", "codegg/write"),
        ("edit", "codegg/edit"),
        ("grep", "codegg/grep"),
        ("glob", "codegg/glob"),
        ("list", "codegg/list"),
        ("git", "codegg/git"),
        ("commit", "codegg/commit"),
        ("lsp", "codegg/lsp"),
        ("security", "codegg/security"),
    ] {
        report.rows.push(ToolBackendReportRow {
            tool: tool.to_string(),
            backend: "Native".to_string(),
            implementation: impl_name.to_string(),
            status: "ready".to_string(),
            raw_mcp_exposed: "n/a".to_string(),
        });
    }

    // --- Per-domain backend config from `tool_backends`. ---
    if let Some(tb) = tool_backends {
        if let Some(lsp) = &tb.lsp {
            let status = match lsp.backend {
                Some(crate::config::schema::ToolImplementationBackendSchema::Disabled) => {
                    "disabled"
                }
                Some(crate::config::schema::ToolImplementationBackendSchema::Mcp) => {
                    if mcp_server_names
                        .map(|names| {
                            names.iter().any(|n| {
                                lsp.server_name
                                    .as_deref()
                                    .map(|s| s == n)
                                    .unwrap_or(false)
                            })
                        })
                        .unwrap_or(false)
                    {
                        "ready"
                    } else {
                        "unavailable"
                    }
                }
                Some(crate::config::schema::ToolImplementationBackendSchema::Builtin) => "ready",
                Some(crate::config::schema::ToolImplementationBackendSchema::Native) | None => "ready",
            };
            // Update the `lsp` row already in the table.
            if let Some(row) = report.rows.iter_mut().find(|r| r.tool == "lsp") {
                row.backend = match lsp.backend {
                    Some(crate::config::schema::ToolImplementationBackendSchema::Mcp) => {
                        "MCP".to_string()
                    }
                    Some(crate::config::schema::ToolImplementationBackendSchema::Builtin) => {
                        "Builtin".to_string()
                    }
                    Some(crate::config::schema::ToolImplementationBackendSchema::Disabled) => {
                        "Disabled".to_string()
                    }
                    Some(crate::config::schema::ToolImplementationBackendSchema::Native) | None => {
                        "Native".to_string()
                    }
                };
                row.implementation = lsp
                    .server_name
                    .clone()
                    .unwrap_or_else(|| "codegg/lsp".to_string());
                row.status = status.to_string();
                row.raw_mcp_exposed = if lsp.expose_raw_mcp_tools.unwrap_or(false) {
                    "yes"
                } else {
                    "no"
                }
                .to_string();
            }
        }
        if let Some(security) = &tb.security {
            if let Some(row) = report.rows.iter_mut().find(|r| r.tool == "security") {
                row.backend = match security.backend {
                    Some(crate::config::schema::ToolImplementationBackendSchema::Mcp) => {
                        "MCP".to_string()
                    }
                    Some(crate::config::schema::ToolImplementationBackendSchema::Builtin) => {
                        "Builtin".to_string()
                    }
                    Some(crate::config::schema::ToolImplementationBackendSchema::Disabled) => {
                        "Disabled".to_string()
                    }
                    Some(crate::config::schema::ToolImplementationBackendSchema::Native) | None => {
                        "Native".to_string()
                    }
                };
                row.implementation = security
                    .server_name
                    .clone()
                    .unwrap_or_else(|| "codegg/security".to_string());
                if matches!(
                    security.backend,
                    Some(crate::config::schema::ToolImplementationBackendSchema::Disabled)
                ) {
                    row.status = "disabled".to_string();
                }
                row.raw_mcp_exposed = if security.expose_raw_mcp_tools.unwrap_or(false) {
                    "yes"
                } else {
                    "no"
                }
                .to_string();
            }
        }
    }

    // --- Warnings ---
    if matches!(
        search_backend,
        crate::config::schema::SearchBackendConfig::Eggsearch
    ) && !eggsearch_present
        && mcp_server_names.is_some()
    {
        report.warnings.push(format!(
            "eggsearch configured but not connected; websearch/webfetch will error unless fallback_to_builtin is enabled (currently {}).",
            if fallback { "on" } else { "off" }
        ));
    }
    if matches!(
        search_backend,
        crate::config::schema::SearchBackendConfig::Disabled
    ) {
        report.warnings
            .push("web search/fetch disabled ([search].backend = \"disabled\").".to_string());
    }
    if !expose_raw
        && matches!(
            search_backend,
            crate::config::schema::SearchBackendConfig::Eggsearch
        )
    {
        report.warnings.push(format!(
            "raw mcp__{}__* hidden because native websearch/webfetch wrappers are active.",
            eggsearch_server
        ));
    }

    report
}

#[cfg(test)]
mod report_tests {
    use super::*;
    use crate::config::schema::{SearchBackendConfig, SearchConfig};

    #[test]
    fn report_renders_table_with_header() {
        let cfg = SearchConfig::default();
        let report = build_report(&cfg, None, None);
        let rendered = report.render();
        assert!(rendered.contains("Tool         Backend"));
        assert!(rendered.contains("websearch"));
        assert!(rendered.contains("webfetch"));
        assert!(rendered.contains("lsp"));
    }

    #[test]
    fn report_eggsearch_unavailable_warning() {
        let cfg = SearchConfig::default();
        let report = build_report(&cfg, None, Some(&[]));
        assert!(report.warnings.iter().any(|w| w.contains("eggsearch")));
    }

    #[test]
    fn report_disabled_backend_marks_disabled_status() {
        let mut cfg = SearchConfig::default();
        cfg.backend = Some(SearchBackendConfig::Disabled);
        let report = build_report(&cfg, None, None);
        let websearch = report.rows.iter().find(|r| r.tool == "websearch").unwrap();
        assert_eq!(websearch.status, "disabled");
        assert!(report.warnings.iter().any(|w| w.contains("disabled")));
    }

    #[test]
    fn report_eggsearch_present_shows_ready() {
        let cfg = SearchConfig::default();
        let names = vec!["eggsearch".to_string()];
        let report = build_report(&cfg, None, Some(&names));
        let websearch = report.rows.iter().find(|r| r.tool == "websearch").unwrap();
        assert_eq!(websearch.status, "ready");
        assert!(websearch.implementation.contains("eggsearch"));
    }

    #[test]
    fn report_hides_raw_mcp_by_default() {
        let cfg = SearchConfig::default();
        let report = build_report(&cfg, None, None);
        let websearch = report.rows.iter().find(|r| r.tool == "websearch").unwrap();
        assert_eq!(websearch.raw_mcp_exposed, "no");
    }
}

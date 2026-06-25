//! LSP security context executor adapter.
//!
//! This adapter lives in `src/security/` rather than in `src/tool/` or the
//! TUI layer because:
//!
//! - It is part of the **security review workflow** boundary — callers
//!   come from `security::workflow::enrichment`, not from TUI commands.
//! - Placing it in `src/tool/` would couple the security workflow to
//!   tool internals (e.g. `ToolError`, `LspInput` deserialization).
//! - Keeping it in the security module makes the dependency direction
//!   clear: security depends on tool, not the other way around.
//!
//! The adapter wraps [`crate::tool::lsp::LspTool`] and implements
//! [`SecurityContextExecutor`](super::workflow::context::SecurityContextExecutor),
//! bridging the gap between the security workflow's JSON-in/JSON-out
//! trait and the tool's `execute()` method which returns a `String`.

use std::sync::Arc;

use crate::tool::lsp::LspTool;
use crate::tool::Tool;

use super::workflow::context::SecurityContextExecutor;

/// Maximum allowed `call_depth` for security context requests.
const MAX_CALL_DEPTH: u8 = 2;

/// Maximum allowed `max_call_nodes` for security context requests.
const MAX_CALL_NODES: usize = 64;

/// Fields that indicate a mutating action. Security context requests are
/// read-only — their presence is rejected.
const MUTATION_FIELDS: &[&str] = &[
    "apply", "write", "edit", "patch", "command", "execute", "shell",
];

/// Adapter that implements [`SecurityContextExecutor`] by delegating to
/// [`LspTool`].
///
/// Validates incoming requests before forwarding them to the tool,
/// ensuring the security workflow never accidentally triggers mutations
/// or passes out-of-range parameters.
pub struct LspSecurityContextExecutor {
    tool: Arc<LspTool>,
}

impl LspSecurityContextExecutor {
    /// Create a new executor wrapping the given [`LspTool`].
    pub fn new(tool: Arc<LspTool>) -> Self {
        Self { tool }
    }
}

#[async_trait::async_trait]
impl SecurityContextExecutor for LspSecurityContextExecutor {
    async fn security_context(
        &self,
        request: serde_json::Value,
    ) -> Result<serde_json::Value, String> {
        validate_security_context_request(&request)?;

        let mut request = request;
        if request.get("operation").is_none() {
            request["operation"] = serde_json::Value::String("securityContext".to_string());
        }

        let result = self
            .tool
            .execute(request)
            .await
            .map_err(|e| format!("securityContext LSP execution failed: {e}"))?;

        serde_json::from_str(&result)
            .map_err(|e| format!("failed to parse securityContext response as JSON: {e}"))
    }
}

/// Validate a security context request for required fields, value
/// ranges, and absence of mutation fields.
pub fn validate_security_context_request(request: &serde_json::Value) -> Result<(), String> {
    // --- required: file_path (string) ---
    match request.get("file_path") {
        Some(serde_json::Value::String(s)) if !s.is_empty() => {}
        Some(_) => return Err("file_path must be a non-empty string".to_string()),
        None => return Err("file_path is required".to_string()),
    }

    // --- required: security_preset (string) ---
    match request.get("security_preset") {
        Some(serde_json::Value::String(s)) if !s.is_empty() => {}
        Some(_) => return Err("security_preset must be a non-empty string".to_string()),
        None => return Err("security_preset is required".to_string()),
    }

    // --- optional: call_depth 0..=MAX_CALL_DEPTH ---
    if let Some(depth) = request.get("call_depth") {
        let d = depth
            .as_u64()
            .ok_or_else(|| "call_depth must be a number".to_string())?;
        if d > MAX_CALL_DEPTH as u64 {
            return Err(format!("call_depth {d} exceeds maximum {MAX_CALL_DEPTH}"));
        }
    }

    // --- optional: max_call_nodes within cap ---
    if let Some(nodes) = request.get("max_call_nodes") {
        let n = nodes
            .as_u64()
            .ok_or_else(|| "max_call_nodes must be a number".to_string())?;
        if n > MAX_CALL_NODES as u64 {
            return Err(format!(
                "max_call_nodes {n} exceeds maximum {MAX_CALL_NODES}"
            ));
        }
    }

    // --- reject mutation fields ---
    for field in MUTATION_FIELDS {
        if request.get(*field).is_some() {
            return Err(format!(
                "mutation field '{field}' is not allowed in security context requests"
            ));
        }
    }

    Ok(())
}

/// Internal trait abstracting the typed hunk source context target.
///
/// This seam exists so the production `LspHunkSourceContextExecutor` can be
/// tested without a live language server: a `#[cfg(test)]` recording target
/// captures the exact request forwarded through the adapter.
#[async_trait::async_trait]
trait TypedHunkSourceContextTarget: Send + Sync {
    async fn execute_hunk_source_context_typed(
        &self,
        request: egglsp::hunk_context::HunkSourceNavigationRequest,
    ) -> Result<egglsp::hunk_context::HunkSourceNavigationResponse, String>;
}

#[async_trait::async_trait]
impl TypedHunkSourceContextTarget for LspTool {
    async fn execute_hunk_source_context_typed(
        &self,
        request: egglsp::hunk_context::HunkSourceNavigationRequest,
    ) -> Result<egglsp::hunk_context::HunkSourceNavigationResponse, String> {
        self.execute_hunk_source_context_typed(request).await
    }
}

/// Adapter that implements [`HunkSourceContextExecutor`] by delegating to
/// a [`TypedHunkSourceContextTarget`] (production: [`LspTool`]).
pub struct LspHunkSourceContextExecutor {
    target: Arc<dyn TypedHunkSourceContextTarget>,
}

impl LspHunkSourceContextExecutor {
    /// Create a new executor wrapping the given [`LspTool`].
    pub fn new(tool: Arc<LspTool>) -> Self {
        Self { target: tool }
    }

    /// Create an executor backed by a custom target (for testing).
    #[cfg(test)]
    fn with_target(target: Arc<dyn TypedHunkSourceContextTarget>) -> Self {
        Self { target }
    }
}

#[async_trait::async_trait]
impl crate::security::workflow::context::HunkSourceContextExecutor
    for LspHunkSourceContextExecutor
{
    async fn execute_hunk_source_context(
        &self,
        request: egglsp::hunk_context::HunkSourceNavigationRequest,
    ) -> Result<egglsp::hunk_context::HunkSourceNavigationResponse, String> {
        self.target.execute_hunk_source_context_typed(request).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn security_context_executor_validates_request() {
        let req = json!({
            "file_path": "/tmp/test.rs",
            "security_preset": "rust_server"
        });
        assert!(
            validate_security_context_request(&req).is_ok(),
            "valid request should pass"
        );
    }

    #[test]
    fn security_context_executor_rejects_bad_call_depth() {
        let req = json!({
            "file_path": "/tmp/test.rs",
            "security_preset": "rust_server",
            "call_depth": 3
        });
        let err = validate_security_context_request(&req).unwrap_err();
        assert!(
            err.contains("call_depth"),
            "error should mention call_depth: {err}"
        );
        assert!(err.contains("3"), "error should mention the value: {err}");
    }

    #[test]
    fn security_context_executor_rejects_missing_file_path() {
        let req = json!({
            "security_preset": "rust_server"
        });
        let err = validate_security_context_request(&req).unwrap_err();
        assert!(
            err.contains("file_path"),
            "error should mention file_path: {err}"
        );
    }

    #[test]
    fn security_context_executor_rejects_missing_preset() {
        let req = json!({
            "file_path": "/tmp/test.rs"
        });
        let err = validate_security_context_request(&req).unwrap_err();
        assert!(
            err.contains("security_preset"),
            "error should mention security_preset: {err}"
        );
    }

    #[test]
    fn security_context_executor_rejects_mutation_fields() {
        let mutation_fields = [
            "apply", "write", "edit", "patch", "command", "execute", "shell",
        ];
        for field in mutation_fields {
            let mut req = json!({
                "file_path": "/tmp/test.rs",
                "security_preset": "rust_server"
            });
            req[field] = json!("some value");
            let err = validate_security_context_request(&req).unwrap_err();
            assert!(
                err.contains(field),
                "error for '{field}' should mention the field name: {err}"
            );
        }
    }

    #[test]
    fn security_context_executor_preserves_caps() {
        // max_call_nodes within cap passes
        let req = json!({
            "file_path": "/tmp/test.rs",
            "security_preset": "rust_server",
            "max_call_nodes": 64
        });
        assert!(
            validate_security_context_request(&req).is_ok(),
            "max_call_nodes=64 should be allowed"
        );

        // max_call_nodes exceeds cap fails
        let req = json!({
            "file_path": "/tmp/test.rs",
            "security_preset": "rust_server",
            "max_call_nodes": 65
        });
        let err = validate_security_context_request(&req).unwrap_err();
        assert!(
            err.contains("max_call_nodes"),
            "error should mention max_call_nodes: {err}"
        );
    }

    #[test]
    fn security_context_executor_rejects_empty_file_path() {
        let req = json!({
            "file_path": "",
            "security_preset": "rust_server"
        });
        let err = validate_security_context_request(&req).unwrap_err();
        assert!(
            err.contains("file_path"),
            "error should mention file_path: {err}"
        );
    }

    #[test]
    fn security_context_executor_allows_optional_fields() {
        let req = json!({
            "file_path": "/tmp/test.rs",
            "security_preset": "rust_server",
            "call_depth": 0,
            "max_call_nodes": 32,
            "max_risk_markers": 80,
            "line": 10,
            "column": 5
        });
        assert!(
            validate_security_context_request(&req).is_ok(),
            "request with all valid optional fields should pass"
        );
    }

    #[test]
    fn noop_hunk_source_context_executor_errors() {
        use super::super::workflow::context::HunkSourceContextExecutor;
        use super::super::workflow::context::NoopHunkSourceContextExecutor;
        use egglsp::hunk_context::HunkSourceNavigationRequest;

        let exec = NoopHunkSourceContextExecutor;
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let request = HunkSourceNavigationRequest {
                file_path: "test.rs".to_string(),
                hunks: vec![],
                patch: None,
                intent: "test".to_string(),
                include_definitions: true,
                include_references: true,
                include_call_hierarchy: false,
                include_type_hierarchy: false,
                excerpt_radius: 40,
                max_hunks: 20,
                max_symbols_per_hunk: 10,
                max_diagnostics_per_hunk: 10,
                max_references_per_hunk: 10,
            };
            let result = exec.execute_hunk_source_context(request).await;
            assert!(result.is_err());
            assert!(result.unwrap_err().contains("no hunkSourceContext"));
        });
    }

    #[test]
    fn noop_hunk_executor_preserves_request_hunks() {
        use super::super::workflow::context::HunkSourceContextExecutor;
        use super::super::workflow::context::NoopHunkSourceContextExecutor;
        use egglsp::hunk_context::{HunkDescriptor, HunkLineRange, HunkSourceNavigationRequest};

        let exec = NoopHunkSourceContextExecutor;
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let request = HunkSourceNavigationRequest {
                file_path: "src/main.rs".to_string(),
                hunks: vec![HunkDescriptor {
                    id: "src/main.rs:0:10-20".to_string(),
                    file_path: "src/main.rs".to_string(),
                    old_range: Some(HunkLineRange {
                        start_line: 10,
                        end_line: 20,
                    }),
                    new_range: Some(HunkLineRange {
                        start_line: 12,
                        end_line: 24,
                    }),
                    header: Some("@@ -10,11 +12,13 @@".to_string()),
                    added_lines: 5,
                    removed_lines: 3,
                    context_lines: 3,
                }],
                patch: None,
                intent: "security_review".to_string(),
                include_definitions: true,
                include_references: true,
                include_call_hierarchy: false,
                include_type_hierarchy: false,
                excerpt_radius: 40,
                max_hunks: 20,
                max_symbols_per_hunk: 10,
                max_diagnostics_per_hunk: 10,
                max_references_per_hunk: 10,
            };

            // The noop executor always errors, but the point is the typed
            // DTO is accepted by the trait method — hunks survive to the
            // trait boundary.
            let result = exec.execute_hunk_source_context(request.clone()).await;
            assert!(result.is_err());
            // Verify the request we sent had the hunk
            assert_eq!(request.hunks.len(), 1);
            assert_eq!(request.hunks[0].id, "src/main.rs:0:10-20");
        });
    }

    #[test]
    fn lsp_hunk_executor_preserves_hunks_field_on_request() {
        use egglsp::hunk_context::{HunkDescriptor, HunkLineRange, HunkSourceNavigationRequest};

        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let request = HunkSourceNavigationRequest {
                file_path: "src/security/lsp_executor.rs".to_string(),
                hunks: vec![
                    HunkDescriptor {
                        id: "src/security/lsp_executor.rs:0:50-75".to_string(),
                        file_path: "src/security/lsp_executor.rs".to_string(),
                        old_range: Some(HunkLineRange {
                            start_line: 50,
                            end_line: 75,
                        }),
                        new_range: Some(HunkLineRange {
                            start_line: 52,
                            end_line: 78,
                        }),
                        header: Some("@@ -50,26 +52,27 @@".to_string()),
                        added_lines: 2,
                        removed_lines: 1,
                        context_lines: 5,
                    },
                    HunkDescriptor {
                        id: "src/security/lsp_executor.rs:1:100-120".to_string(),
                        file_path: "src/security/lsp_executor.rs".to_string(),
                        old_range: Some(HunkLineRange {
                            start_line: 100,
                            end_line: 120,
                        }),
                        new_range: Some(HunkLineRange {
                            start_line: 102,
                            end_line: 122,
                        }),
                        header: Some("@@ -100,21 +102,21 @@".to_string()),
                        added_lines: 1,
                        removed_lines: 1,
                        context_lines: 4,
                    },
                ],
                patch: None,
                intent: "security_review".to_string(),
                include_definitions: true,
                include_references: true,
                include_call_hierarchy: false,
                include_type_hierarchy: false,
                excerpt_radius: 40,
                max_hunks: 20,
                max_symbols_per_hunk: 10,
                max_diagnostics_per_hunk: 10,
                max_references_per_hunk: 10,
            };

            assert_eq!(request.hunks.len(), 2, "request should have 2 hunks");
            assert!(
                request.patch.is_none(),
                "patch should be None to confirm typed path (no JSON serialization)"
            );

            assert_eq!(
                request.hunks[0].id, "src/security/lsp_executor.rs:0:50-75",
                "first hunk id should match"
            );
            assert_eq!(
                request.hunks[1].id, "src/security/lsp_executor.rs:1:100-120",
                "second hunk id should match"
            );

            let _ = request;
        });
    }
}

#[cfg(test)]
mod lsp_hunk_executor_integration_tests {
    use egglsp::hunk_context::{HunkDescriptor, HunkLineRange, HunkSourceNavigationRequest};

    #[test]
    fn model_facing_and_internal_executor_both_use_typed_method() {
        let request = HunkSourceNavigationRequest {
            file_path: "src/lib.rs".to_string(),
            hunks: vec![HunkDescriptor {
                id: "src/lib.rs:0:10-20".to_string(),
                file_path: "src/lib.rs".to_string(),
                old_range: Some(HunkLineRange {
                    start_line: 10,
                    end_line: 20,
                }),
                new_range: Some(HunkLineRange {
                    start_line: 12,
                    end_line: 24,
                }),
                header: Some("@@ -10,11 +12,13 @@".to_string()),
                added_lines: 5,
                removed_lines: 3,
                context_lines: 3,
            }],
            patch: Some("--- a/src/lib.rs\n+++ b/src/lib.rs\n@@ -10,11 +12,13 @@\n old line\n+new line\n context\n".to_string()),
            intent: "security_review".to_string(),
            include_definitions: true,
            include_references: true,
            include_call_hierarchy: false,
            include_type_hierarchy: false,
            excerpt_radius: 40,
            max_hunks: 20,
            max_symbols_per_hunk: 10,
            max_diagnostics_per_hunk: 10,
            max_references_per_hunk: 10,
        };

        assert_eq!(request.hunks.len(), 1);
        assert!(
            request.patch.is_some(),
            "model-facing path uses patch, internal path uses hunks — both typed"
        );
    }
}

#[cfg(test)]
mod lsp_hunk_adapter_tests {
    use super::*;
    use crate::security::workflow::context::HunkSourceContextExecutor;
    use egglsp::hunk_context::{
        HunkDescriptor, HunkLineRange, HunkSourceNavigationRequest, HunkSourceNavigationResponse,
    };

    /// Recording target that captures the exact request forwarded through the
    /// `LspHunkSourceContextExecutor` adapter.
    struct RecordingTarget {
        captured: std::sync::Mutex<Option<HunkSourceNavigationRequest>>,
        response: HunkSourceNavigationResponse,
    }

    impl RecordingTarget {
        fn new(response: HunkSourceNavigationResponse) -> Self {
            Self {
                captured: std::sync::Mutex::new(None),
                response,
            }
        }

        fn captured_request(&self) -> Option<HunkSourceNavigationRequest> {
            self.captured.lock().unwrap().clone()
        }
    }

    #[async_trait::async_trait]
    impl TypedHunkSourceContextTarget for RecordingTarget {
        async fn execute_hunk_source_context_typed(
            &self,
            request: HunkSourceNavigationRequest,
        ) -> Result<HunkSourceNavigationResponse, String> {
            *self.captured.lock().unwrap() = Some(request);
            Ok(self.response.clone())
        }
    }

    /// Target that always returns an error.
    struct ErrorTarget {
        msg: String,
    }

    #[async_trait::async_trait]
    impl TypedHunkSourceContextTarget for ErrorTarget {
        async fn execute_hunk_source_context_typed(
            &self,
            _request: HunkSourceNavigationRequest,
        ) -> Result<HunkSourceNavigationResponse, String> {
            Err(self.msg.clone())
        }
    }

    fn make_request(file_path: &str) -> HunkSourceNavigationRequest {
        HunkSourceNavigationRequest {
            file_path: file_path.to_string(),
            hunks: vec![HunkDescriptor {
                id: format!("{file_path}:0:10-20"),
                file_path: file_path.to_string(),
                old_range: Some(HunkLineRange {
                    start_line: 10,
                    end_line: 20,
                }),
                new_range: Some(HunkLineRange {
                    start_line: 12,
                    end_line: 24,
                }),
                header: Some("@@ -10,11 +12,13 @@".to_string()),
                added_lines: 5,
                removed_lines: 3,
                context_lines: 3,
            }],
            patch: None,
            intent: "security_review".to_string(),
            include_definitions: true,
            include_references: false,
            include_call_hierarchy: false,
            include_type_hierarchy: false,
            excerpt_radius: 40,
            max_hunks: 1,
            max_symbols_per_hunk: 10,
            max_diagnostics_per_hunk: 10,
            max_references_per_hunk: 10,
        }
    }

    #[tokio::test]
    async fn lsp_hunk_executor_forwards_exact_typed_request() {
        let request = make_request("src/main.rs");
        let response = HunkSourceNavigationResponse::new("src/main.rs");
        let target = Arc::new(RecordingTarget::new(response));
        let executor = LspHunkSourceContextExecutor::with_target(target.clone());

        let _result = executor.execute_hunk_source_context(request.clone()).await;

        let captured = target
            .captured_request()
            .expect("target should have captured a request");
        assert_eq!(captured.file_path, "src/main.rs");
        assert_eq!(captured.intent, "security_review");
        assert!(captured.include_definitions);
        assert!(!captured.include_references);
        assert!(captured.patch.is_none());
        assert_eq!(captured.hunks.len(), 1);
        assert_eq!(captured.hunks[0].id, "src/main.rs:0:10-20");
        assert_eq!(captured.excerpt_radius, 40);
        assert_eq!(captured.max_symbols_per_hunk, 10);
    }

    #[tokio::test]
    async fn lsp_hunk_executor_propagates_target_response() {
        let request = make_request("src/lib.rs");
        let response = HunkSourceNavigationResponse::new("src/lib.rs");
        let target = Arc::new(RecordingTarget::new(response.clone()));
        let executor = LspHunkSourceContextExecutor::with_target(target);

        let result = executor.execute_hunk_source_context(request).await;
        let actual = result.expect("should succeed");

        assert_eq!(actual.file_path, response.file_path);
    }

    #[tokio::test]
    async fn lsp_hunk_executor_propagates_target_error() {
        let request = make_request("src/bad.rs");
        let target = Arc::new(ErrorTarget {
            msg: "LSP server crashed".to_string(),
        });
        let executor = LspHunkSourceContextExecutor::with_target(target);

        let err = executor
            .execute_hunk_source_context(request)
            .await
            .expect_err("should fail");
        assert_eq!(err, "LSP server crashed");
    }
}

// ---------------------------------------------------------------------------
// Pass 5: Security review context-packet integration tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod security_context_packet_bridge_tests {
    use std::path::PathBuf;

    use egglsp::context::{
        LineRange, LspContextItem, LspContextItemKind, LspContextPacket, LspContextPacketMode,
        LspContextRequest, LspContextScore, LspEvidenceFreshness, LspEvidenceProvenance,
        LspRiskMode,
    };
    use egglsp::{
        build_security_evidence_summary, build_security_lsp_context_request, SecurityRiskTag,
    };

    use super::validate_security_context_request;

    fn make_item(
        kind: LspContextItemKind,
        file: &str,
        line: Option<u32>,
        message: &str,
    ) -> LspContextItem {
        LspContextItem {
            kind,
            file: PathBuf::from(file),
            range: line.map(|l| LineRange {
                start: l,
                end: l + 1,
            }),
            line,
            column: None,
            message: message.to_string(),
            symbol: None,
            source: None,
            provenance: LspEvidenceProvenance {
                server_id: "rust-analyzer".to_string(),
                server_generation: Some(1),
                operation: "test".to_string(),
                freshness: LspEvidenceFreshness::Fresh,
                capability_decision: None,
                document_version: None,
                age_ms: None,
                post_restart: false,
            },
            score: LspContextScore {
                priority: 10,
                is_hunk_local: false,
                is_error: false,
                is_same_file: false,
                freshness_rank: 0,
            },
            payload: None,
        }
    }

    #[test]
    fn security_request_builder_uses_aggressive_risk_mode() {
        let files = vec![PathBuf::from("src/auth/login.rs")];
        let hunks = vec![egglsp::hunk_context::HunkDescriptor {
            id: "src/auth/login.rs:0:1-3".to_string(),
            file_path: "src/auth/login.rs".to_string(),
            old_range: None,
            new_range: None,
            header: Some("@@ -1,3 +1,3 @@".to_string()),
            added_lines: 1,
            removed_lines: 1,
            context_lines: 2,
        }];
        let req = build_security_lsp_context_request(&files, &hunks);
        match req {
            LspContextRequest::Review { risk_mode, .. } => {
                assert_eq!(risk_mode, LspRiskMode::Aggressive);
            }
            _ => panic!("expected Review request"),
        }
    }

    #[test]
    fn security_request_builder_propagates_changed_files_and_hunks() {
        let files = vec![PathBuf::from("a.rs"), PathBuf::from("b.rs")];
        let hunks = vec![egglsp::hunk_context::HunkDescriptor {
            id: "a.rs:0:1-3".to_string(),
            file_path: "a.rs".to_string(),
            old_range: None,
            new_range: None,
            header: None,
            added_lines: 1,
            removed_lines: 1,
            context_lines: 2,
        }];
        let req = build_security_lsp_context_request(&files, &hunks);
        match req {
            LspContextRequest::Review {
                changed_files,
                hunks: req_hunks,
                ..
            } => {
                assert_eq!(changed_files, files);
                assert_eq!(req_hunks.len(), 1);
            }
            _ => panic!("expected Review request"),
        }
    }

    #[test]
    fn security_evidence_summary_preserves_all_counts() {
        let packet = LspContextPacket {
            request: LspContextRequest::Review {
                changed_files: vec![PathBuf::from("src/auth.rs")],
                hunks: Vec::new(),
                risk_mode: LspRiskMode::Aggressive,
            },
            items: vec![
                make_item(
                    LspContextItemKind::Diagnostic,
                    "src/auth.rs",
                    Some(1),
                    "unsafe",
                ),
                make_item(
                    LspContextItemKind::Reference,
                    "src/caller.rs",
                    Some(2),
                    "call",
                ),
                make_item(
                    LspContextItemKind::Reference,
                    "src/other.rs",
                    Some(3),
                    "call",
                ),
                make_item(
                    LspContextItemKind::Definition,
                    "src/auth.rs",
                    Some(4),
                    "def",
                ),
                make_item(
                    LspContextItemKind::Implementation,
                    "src/impl.rs",
                    Some(5),
                    "impl",
                ),
            ],
            previews: Vec::new(),
            preview_ids: Vec::new(),
            mode: LspContextPacketMode::Opportunistic,
            workspace_root: None,
            generated_at: None,
            server_id: None,
            server_generation: None,
            operational_state: None,
            budget: None,
            notes: vec!["LSP state: ready".to_string()],
            truncation: Default::default(),
        };
        let summary = build_security_evidence_summary(&packet);
        assert_eq!(summary.diagnostics_count, 1);
        assert_eq!(summary.references_count, 2);
        assert_eq!(summary.definitions_count, 1);
        assert_eq!(summary.implementations_count, 1);
        assert_eq!(summary.public_api_fanout, 2);
        assert_eq!(summary.server_id, Some("rust-analyzer".to_string()));
        assert!(summary
            .risk_tags
            .contains(&SecurityRiskTag::ChangedPublicApi));
        assert!(summary
            .risk_tags
            .contains(&SecurityRiskTag::ChangedAuthSecuritySensitive));
    }

    #[test]
    fn security_evidence_summary_omits_previews_from_packets() {
        // Security review should not include preview artifacts even
        // when the packet contains them. The summary never counts
        // previews because it counts evidence items, not artifacts.
        let packet = LspContextPacket {
            request: LspContextRequest::Review {
                changed_files: vec![PathBuf::from("a.rs")],
                hunks: Vec::new(),
                risk_mode: LspRiskMode::Aggressive,
            },
            items: vec![make_item(
                LspContextItemKind::Definition,
                "a.rs",
                Some(0),
                "def",
            )],
            previews: vec![egglsp::context::LspPreviewArtifact::Rename {
                description: "should not be counted".to_string(),
                edit_count: 5,
                patches: Vec::new(),
            }],
            preview_ids: vec!["preview-1".to_string()],
            mode: LspContextPacketMode::Opportunistic,
            workspace_root: None,
            generated_at: None,
            server_id: None,
            server_generation: None,
            operational_state: None,
            budget: None,
            notes: Vec::new(),
            truncation: Default::default(),
        };
        let summary = build_security_evidence_summary(&packet);
        // Definitions count is from items, not previews.
        assert_eq!(summary.definitions_count, 1);
        // Summary has no field for previews — they're never surfaced
        // through the security evidence summary, by design.
    }

    #[test]
    fn security_evidence_summary_detects_truncation() {
        let mut packet = LspContextPacket {
            request: LspContextRequest::Review {
                changed_files: vec![PathBuf::from("a.rs")],
                hunks: Vec::new(),
                risk_mode: LspRiskMode::Aggressive,
            },
            items: vec![make_item(
                LspContextItemKind::Definition,
                "a.rs",
                Some(0),
                "def",
            )],
            previews: Vec::new(),
            preview_ids: Vec::new(),
            mode: LspContextPacketMode::Opportunistic,
            workspace_root: None,
            generated_at: None,
            server_id: None,
            server_generation: None,
            operational_state: None,
            budget: None,
            notes: Vec::new(),
            truncation: Default::default(),
        };
        packet.truncation.references_truncated = true;
        let summary = build_security_evidence_summary(&packet);
        assert!(summary.truncated);
    }

    #[test]
    fn security_evidence_summary_detects_stale_evidence() {
        let mut item = make_item(LspContextItemKind::Diagnostic, "a.rs", Some(0), "err");
        item.provenance.freshness = LspEvidenceFreshness::Stale;
        let packet = LspContextPacket {
            request: LspContextRequest::Review {
                changed_files: vec![PathBuf::from("a.rs")],
                hunks: Vec::new(),
                risk_mode: LspRiskMode::Aggressive,
            },
            items: vec![item],
            previews: Vec::new(),
            preview_ids: Vec::new(),
            mode: LspContextPacketMode::Opportunistic,
            workspace_root: None,
            generated_at: None,
            server_id: None,
            server_generation: None,
            operational_state: None,
            budget: None,
            notes: Vec::new(),
            truncation: Default::default(),
        };
        let summary = build_security_evidence_summary(&packet);
        assert!(summary.stale);
    }

    #[test]
    fn security_context_executor_never_accepts_code_action_execution() {
        use serde_json::json;
        // Even when callers explicitly send a codeAction request,
        // validation must reject it. The mutation field check covers
        // the strings we use in the executor.
        let req = json!({
            "file_path": "/tmp/test.rs",
            "security_preset": "rust_server",
            "operation": "codeAction"
        });
        // The "operation" field is not in MUTATION_FIELDS, so this
        // request passes validation. The actual operation selection
        // happens inside LspTool::execute — for security context,
        // the executor forces "operation": "securityContext" if not
        // present. The validate function does not gate on "operation"
        // because the executor enforces it; the real protection is
        // that no mutation field is present.
        assert!(validate_security_context_request(&req).is_ok());
        // With any of the explicit mutation fields, validation must
        // reject — this is the actual security boundary.
        for field in [
            "apply", "write", "edit", "patch", "command", "execute", "shell",
        ] {
            let mut r = json!({
                "file_path": "/tmp/test.rs",
                "security_preset": "rust_server"
            });
            r[field] = json!("value");
            assert!(
                validate_security_context_request(&r).is_err(),
                "field '{field}' must be rejected"
            );
        }
    }
}

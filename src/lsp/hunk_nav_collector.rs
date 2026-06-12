use crate::lsp::hunk_nav::HunkSourceNavigator;
use crate::lsp::hunk_nav_parser::parse_unified_diff;
use crate::lsp::semantic_context::SemanticContextCollector;
use egglsp::hunk_context::{
    HunkSourceNavigationLimits, HunkSourceNavigationRequest, HunkSourceNavigationResponse,
};
use egglsp::semantic_context::SemanticContextRequest;

pub struct HunkSourceNavigationCollector {
    semantic_collector: SemanticContextCollector,
    navigator: HunkSourceNavigator,
}

impl HunkSourceNavigationCollector {
    pub fn new(
        semantic_collector: SemanticContextCollector,
        navigator: HunkSourceNavigator,
    ) -> Self {
        Self {
            semantic_collector,
            navigator,
        }
    }

    pub async fn collect(
        &self,
        request: HunkSourceNavigationRequest,
    ) -> Result<HunkSourceNavigationResponse, String> {
        let mut hunks = request.hunks;

        if let Some(patch) = &request.patch {
            let parsed = parse_unified_diff(patch)
                .map_err(|e| format!("hunkSourceContext: patch parse error: {e}"))?;
            hunks.extend(parsed);
        }

        if hunks.is_empty() {
            return Err(
                "hunkSourceContext: no hunks provided (patch parsed to empty or no hunks supplied)"
                    .to_string(),
            );
        }

        hunks.truncate(request.max_hunks);

        let intent = match request.intent.as_str() {
            "security_review" => egglsp::semantic_context::SemanticContextIntent::SecurityReview,
            "explain" => egglsp::semantic_context::SemanticContextIntent::Explain,
            "review" => egglsp::semantic_context::SemanticContextIntent::Review,
            _ => egglsp::semantic_context::SemanticContextIntent::Navigation,
        };

        let first_hunk_line = hunks
            .first()
            .and_then(|h| h.new_range.as_ref().map(|r| r.start_line));

        let mut semantic_request = SemanticContextRequest::new(&request.file_path, intent)
            .with_excerpt_radius(request.excerpt_radius);

        if let Some(line) = first_hunk_line {
            semantic_request = semantic_request.with_position(line, 1);
        }

        semantic_request.include_definitions = request.include_definitions;
        semantic_request.include_references = request.include_references;
        semantic_request.include_call_hierarchy = request.include_call_hierarchy;
        semantic_request.include_type_hierarchy = request.include_type_hierarchy;
        semantic_request.include_overlay = false;
        semantic_request.include_source_actions = false;

        let semantic = self
            .semantic_collector
            .collect(semantic_request)
            .await
            .map_err(|e| format!("hunkSourceContext: semantic collect: {e}"))?;

        let mut limits = HunkSourceNavigationLimits::default();
        if hunks.len() >= request.max_hunks {
            limits.hunks_truncated = true;
        }

        let mut response = self.navigator.build(semantic, hunks);
        response.limits.hunks_truncated = response.limits.hunks_truncated || limits.hunks_truncated;
        response.truncated = response.limits.hunks_truncated
            || response.limits.symbols_truncated
            || response.limits.diagnostics_truncated
            || response.limits.references_truncated
            || response.limits.excerpt_truncated;

        Ok(response)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lsp::diagnostics::DiagnosticsCollector;
    use crate::lsp::operations::LspOperations;
    use crate::lsp::service::LspService;
    use egglsp::config::LspConfig;

    use std::path::PathBuf;
    use std::sync::Arc;

    fn make_collector() -> HunkSourceNavigationCollector {
        let service = Arc::new(LspService::new(LspConfig::default()));
        let operations = Arc::new(LspOperations::new(service.clone()));
        let diagnostics = Arc::new(DiagnosticsCollector::new(service.clone()));
        let sem_collector =
            SemanticContextCollector::new(service, operations, diagnostics, PathBuf::from("/tmp"));
        let nav = HunkSourceNavigator::new();
        HunkSourceNavigationCollector::new(sem_collector, nav)
    }

    #[tokio::test]
    async fn empty_hunks_returns_error() {
        let collector = make_collector();
        let request = HunkSourceNavigationRequest {
            file_path: "test.rs".to_string(),
            hunks: vec![],
            patch: None,
            intent: "navigation".to_string(),
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
        let result = collector.collect(request).await;
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("no hunks"));
    }

    #[tokio::test]
    async fn patch_parsed_into_hunks() {
        let collector = make_collector();
        let patch = "\
diff --git a/src/main.rs b/src/main.rs
--- a/src/main.rs
+++ b/src/main.rs
@@ -10,6 +10,8 @@ fn main() {
     let x = 1;
     let y = 2;
+    let z = 3;
+    let w = 4;
     println!(\"{x} {y}\");
 }";
        let request = HunkSourceNavigationRequest {
            file_path: "test.rs".to_string(),
            hunks: vec![],
            patch: Some(patch.to_string()),
            intent: "navigation".to_string(),
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
        let result = collector.collect(request).await;
        // The semantic collector will fail because test.rs doesn't exist,
        // but the patch parsing should succeed and we should get past that stage.
        // If the error is about file not found, patch parsing worked.
        match result {
            Ok(resp) => {
                assert!(!resp.hunks.is_empty());
            }
            Err(e) => {
                // Patch parsing succeeded; collector failed on file resolution
                assert!(
                    !e.contains("patch parse error"),
                    "patch should have parsed: {e}"
                );
            }
        }
    }

    #[tokio::test]
    async fn malformed_patch_returns_error() {
        let collector = make_collector();
        let request = HunkSourceNavigationRequest {
            file_path: "test.rs".to_string(),
            hunks: vec![],
            patch: Some("not a diff".to_string()),
            intent: "navigation".to_string(),
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
        let result = collector.collect(request).await;
        // Empty diff returns EmptyInput error
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(
            err.contains("patch parse error") || err.contains("no hunks"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn hunk_source_navigation_collector_constructs() {
        let _collector = make_collector();
    }
}

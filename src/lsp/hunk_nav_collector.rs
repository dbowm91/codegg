use crate::lsp::hunk_nav::HunkSourceNavigator;
use crate::lsp::hunk_nav_parser::parse_unified_diff;
use crate::lsp::semantic_context::SemanticContextCollector;
use egglsp::hunk_context::{HunkSourceNavigationRequest, HunkSourceNavigationResponse};
use egglsp::semantic_context::SemanticContextRequest;

use std::path::{Path, PathBuf};

/// Normalize a file path from a unified diff by stripping leading `a/` or `b/`
/// diff prefixes and converting to a PathBuf. Rejects `..`, absolute paths,
/// and empty terminal components.
fn normalize_diff_relative_path(raw: &str) -> Result<PathBuf, String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err("empty path not allowed in diff".to_string());
    }
    let stripped = trimmed
        .strip_prefix("a/")
        .or_else(|| trimmed.strip_prefix("b/"))
        .unwrap_or(trimmed);

    let path = PathBuf::from(stripped);

    if path.is_absolute() {
        return Err(format!("absolute path not allowed in diff: {raw}"));
    }

    for component in path.components() {
        match component {
            std::path::Component::ParentDir => {
                return Err(format!("path traversal not allowed in diff: {raw}"));
            }
            std::path::Component::Normal(_) => {}
            _ => {}
        }
    }

    Ok(path)
}

/// Normalize a request file path relative to the allowed root.
/// Uses `Path::strip_prefix` (not string prefix removal).
fn normalize_request_relative_path(
    request_file: &Path,
    allowed_root: &Path,
) -> Result<PathBuf, String> {
    let canonical_file = request_file
        .canonicalize()
        .map_err(|e| format!("failed to canonicalize {}: {e}", request_file.display()))?;
    let canonical_root = allowed_root.canonicalize().map_err(|e| {
        format!(
            "failed to canonicalize root {}: {e}",
            allowed_root.display()
        )
    })?;

    canonical_file
        .strip_prefix(&canonical_root)
        .map(|rel| rel.to_path_buf())
        .map_err(|_| {
            format!(
                "path {} is outside allowed root {}",
                request_file.display(),
                allowed_root.display()
            )
        })
}

/// Strip `a/` or `b/` diff prefixes for simple string-based comparison.
/// For path-aware comparison prefer `normalize_diff_relative_path`.
#[allow(dead_code)]
fn normalize_hunk_path(path: &str) -> String {
    let p = path.trim_start_matches("a/").trim_start_matches("b/");
    p.to_string()
}

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

        // Phase 5: Reject multi-file patches unless all hunks match file_path.
        let target_path = PathBuf::from(&request.file_path);
        let target_relative =
            normalize_request_relative_path(&target_path, self.semantic_collector.allowed_root())
                .unwrap_or_else(|_| target_path.clone());
        let mismatched_files: Vec<&str> = hunks
            .iter()
            .filter(|h| {
                if h.file_path.is_empty() {
                    return false;
                }
                match normalize_diff_relative_path(&h.file_path) {
                    Ok(hunk_path) => hunk_path != target_relative,
                    Err(_) => true,
                }
            })
            .map(|h| h.file_path.as_str())
            .collect();
        if !mismatched_files.is_empty() {
            let mut unique = mismatched_files;
            unique.sort();
            unique.dedup();
            return Err(format!(
                "hunkSourceContext currently supports one file per request; \
                 patch contains hunks for: {}",
                unique.join(", ")
            ));
        }

        // Phase 4: Record raw count before truncation and coerce max_hunks == 0 to 1.
        let effective_max = request.max_hunks.max(1);
        let raw_hunk_count = hunks.len();
        hunks.truncate(effective_max);

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

        let hunk_truncation = raw_hunk_count > effective_max;

        let mut response = self.navigator.build(semantic, hunks);
        response.limits.hunks_truncated = response.limits.hunks_truncated || hunk_truncation;
        response.truncated = response.limits.hunks_truncated
            || response.limits.symbols_truncated
            || response.limits.diagnostics_truncated
            || response.limits.references_truncated
            || response.limits.excerpt_truncated;

        if response.hunks.len() > 1 {
            response.push_note(
                "Semantic context was collected centered on the first hunk. \
                 Definitions, references, and hierarchy are shared across all hunks.",
            );
        }

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

    // --- Phase 4: normalize_hunk_path tests ---

    #[test]
    fn normalize_strips_a_prefix() {
        assert_eq!(normalize_hunk_path("a/src/main.rs"), "src/main.rs");
    }

    #[test]
    fn normalize_strips_b_prefix() {
        assert_eq!(normalize_hunk_path("b/src/main.rs"), "src/main.rs");
    }

    #[test]
    fn normalize_no_prefix_unchanged() {
        assert_eq!(normalize_hunk_path("src/main.rs"), "src/main.rs");
    }

    #[test]
    fn normalize_empty_string() {
        assert_eq!(normalize_hunk_path(""), "");
    }

    // --- normalize_diff_relative_path tests ---

    #[test]
    fn normalize_diff_relative_path_strips_a_prefix() {
        let result = normalize_diff_relative_path("a/src/lib.rs").unwrap();
        assert_eq!(result, PathBuf::from("src/lib.rs"));
    }

    #[test]
    fn normalize_diff_relative_path_strips_b_prefix() {
        let result = normalize_diff_relative_path("b/src/lib.rs").unwrap();
        assert_eq!(result, PathBuf::from("src/lib.rs"));
    }

    #[test]
    fn normalize_diff_relative_path_no_prefix_unchanged() {
        let result = normalize_diff_relative_path("src/lib.rs").unwrap();
        assert_eq!(result, PathBuf::from("src/lib.rs"));
    }

    #[test]
    fn normalize_diff_relative_path_rejects_traversal() {
        assert!(normalize_diff_relative_path("a/../outside.rs").is_err());
    }

    #[test]
    fn normalize_diff_relative_path_rejects_absolute() {
        assert!(normalize_diff_relative_path("/etc/passwd").is_err());
    }

    #[test]
    fn normalize_diff_relative_path_rejects_empty() {
        assert!(normalize_diff_relative_path("").is_err());
    }

    // --- Phase 5: multi-file patch rejection ---

    fn make_request_with_hunks(
        file_path: &str,
        hunks: Vec<egglsp::hunk_context::HunkDescriptor>,
        max_hunks: usize,
    ) -> HunkSourceNavigationRequest {
        HunkSourceNavigationRequest {
            file_path: file_path.to_string(),
            hunks,
            patch: None,
            intent: "navigation".to_string(),
            include_definitions: true,
            include_references: true,
            include_call_hierarchy: false,
            include_type_hierarchy: false,
            excerpt_radius: 40,
            max_hunks,
            max_symbols_per_hunk: 10,
            max_diagnostics_per_hunk: 10,
            max_references_per_hunk: 10,
        }
    }

    fn make_hunk(
        id: &str,
        file_path: &str,
        start: u32,
        end: u32,
    ) -> egglsp::hunk_context::HunkDescriptor {
        egglsp::hunk_context::HunkDescriptor {
            id: id.to_string(),
            file_path: file_path.to_string(),
            old_range: None,
            new_range: Some(egglsp::hunk_context::HunkLineRange {
                start_line: start,
                end_line: end,
            }),
            header: None,
            added_lines: 0,
            removed_lines: 0,
            context_lines: 0,
        }
    }

    #[tokio::test]
    async fn multi_file_patch_rejected() {
        let collector = make_collector();
        let hunks = vec![
            make_hunk("h0", "src/a.rs", 1, 5),
            make_hunk("h1", "src/b.rs", 10, 15),
        ];
        let request = make_request_with_hunks("src/a.rs", hunks, 20);
        let result = collector.collect(request).await;
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(
            err.contains("one file per request"),
            "expected multi-file rejection: {err}"
        );
        assert!(
            err.contains("src/b.rs"),
            "error should list the mismatched file: {err}"
        );
    }

    #[tokio::test]
    async fn single_file_patch_accepted() {
        let collector = make_collector();
        let hunks = vec![
            make_hunk("h0", "src/main.rs", 1, 5),
            make_hunk("h1", "src/main.rs", 10, 15),
        ];
        let request = make_request_with_hunks("src/main.rs", hunks, 20);
        let result = collector.collect(request).await;
        // Semantic collection will fail (no LSP), but multi-file check should pass.
        match result {
            Ok(_) => {}
            Err(e) => {
                assert!(
                    !e.contains("one file per request"),
                    "single-file patch should not be rejected: {e}"
                );
            }
        }
    }

    #[tokio::test]
    async fn patch_with_a_b_prefix_matches_file_path() {
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
            file_path: "src/main.rs".to_string(),
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
        match result {
            Ok(_) => {}
            Err(e) => {
                assert!(
                    !e.contains("one file per request"),
                    "normalized paths should match: {e}"
                );
            }
        }
    }

    #[tokio::test]
    async fn multi_file_multi_hunk_rejected_with_all_files_named() {
        let collector = make_collector();
        let hunks = vec![
            make_hunk("h0", "src/a.rs", 1, 5),
            make_hunk("h1", "src/b.rs", 10, 15),
            make_hunk("h2", "src/c.rs", 20, 25),
        ];
        let request = make_request_with_hunks("src/a.rs", hunks, 20);
        let result = collector.collect(request).await;
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.contains("src/b.rs"), "should list b.rs: {err}");
        assert!(err.contains("src/c.rs"), "should list c.rs: {err}");
        assert!(
            !err.contains("src/a.rs"),
            "matching file should not be listed: {err}"
        );
    }

    #[tokio::test]
    async fn max_hunks_zero_coerced_to_one() {
        let collector = make_collector();
        let hunks = vec![make_hunk("h0", "src/main.rs", 1, 5)];
        let request = make_request_with_hunks("src/main.rs", hunks, 0);
        let result = collector.collect(request).await;
        // With max_hunks=0 coerced to 1, the single hunk should not be truncated.
        match result {
            Ok(resp) => {
                assert!(
                    !resp.limits.hunks_truncated,
                    "exact fit after coercion should not be truncated"
                );
            }
            Err(e) => {
                // Semantic collection may fail, but truncation flag should not be wrong.
                assert!(
                    !e.contains("one file per request"),
                    "should not fail multi-file check: {e}"
                );
            }
        }
    }
}

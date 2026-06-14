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

    if stripped.is_empty() {
        return Err(format!(
            "diff path resolves to empty after stripping prefix: {raw}"
        ));
    }

    let mut normalized = PathBuf::new();
    for component in Path::new(stripped).components() {
        match component {
            std::path::Component::Normal(part) => normalized.push(part),
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir
            | std::path::Component::RootDir
            | std::path::Component::Prefix(_) => {
                return Err(format!(
                    "path traversal or absolute component not allowed in diff: {raw}"
                ));
            }
        }
    }
    if normalized.as_os_str().is_empty() {
        return Err(format!("diff path has no normal components: {raw}"));
    }
    Ok(normalized)
}

/// Normalize a request file path relative to the allowed root.
/// Uses `Path::canonicalize` for symlink-safe comparison and rejects
/// paths outside the root or paths that resolve to the root itself.
fn normalize_request_relative_path(
    request_file: &Path,
    allowed_root: &Path,
) -> Result<PathBuf, String> {
    let canonical_root = allowed_root.canonicalize().map_err(|e| {
        format!(
            "failed to canonicalize root {}: {e}",
            allowed_root.display()
        )
    })?;

    let resolved_file = if request_file.is_absolute() {
        request_file.to_path_buf()
    } else {
        canonical_root.join(request_file)
    };

    let canonical_file = resolved_file
        .canonicalize()
        .map_err(|e| format!("failed to canonicalize {}: {e}", resolved_file.display()))?;

    let relative = canonical_file.strip_prefix(&canonical_root).map_err(|_| {
        format!(
            "path {} is outside allowed root {}",
            canonical_file.display(),
            canonical_root.display()
        )
    })?;

    if relative.as_os_str().is_empty() {
        return Err("request file resolves to the project root, not a file".to_string());
    }

    Ok(relative.to_path_buf())
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
                .map_err(|e| format!("hunkSourceContext: invalid file path: {e}"))?;
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

    fn make_collector() -> (HunkSourceNavigationCollector, tempfile::TempDir) {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("project");
        std::fs::create_dir_all(&root).unwrap();
        // Create files needed by multi-file tests
        for f in ["src/a.rs", "src/b.rs", "src/c.rs", "src/main.rs"] {
            let p = root.join(f);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(&p, "fn placeholder() {}\n").unwrap();
        }
        let service = Arc::new(LspService::new(LspConfig::default()));
        let operations = Arc::new(LspOperations::new(service.clone()));
        let diagnostics = Arc::new(DiagnosticsCollector::new(service.clone()));
        let sem_collector = SemanticContextCollector::new(service, operations, diagnostics, root);
        let nav = HunkSourceNavigator::new();
        (HunkSourceNavigationCollector::new(sem_collector, nav), temp)
    }

    #[tokio::test]
    async fn empty_hunks_returns_error() {
        let (collector, _temp) = make_collector();
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
        let (collector, _temp) = make_collector();
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
        let (collector, _temp) = make_collector();
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
        let (_collector, _temp) = make_collector();
    }

    // --- normalize_diff_relative_path tests ---

    #[test]
    fn normalize_diff_a_prefix() {
        assert_eq!(
            normalize_diff_relative_path("a/src/lib.rs").unwrap(),
            PathBuf::from("src/lib.rs")
        );
    }

    #[test]
    fn normalize_diff_b_prefix() {
        assert_eq!(
            normalize_diff_relative_path("b/src/lib.rs").unwrap(),
            PathBuf::from("src/lib.rs")
        );
    }

    #[test]
    fn normalize_diff_no_prefix() {
        assert_eq!(
            normalize_diff_relative_path("src/lib.rs").unwrap(),
            PathBuf::from("src/lib.rs")
        );
    }

    #[test]
    fn normalize_diff_cur_dir() {
        assert_eq!(
            normalize_diff_relative_path("./src/lib.rs").unwrap(),
            PathBuf::from("src/lib.rs")
        );
    }

    #[test]
    fn normalize_diff_rejects_empty_a() {
        assert!(normalize_diff_relative_path("a/").is_err());
    }

    #[test]
    fn normalize_diff_rejects_empty_b() {
        assert!(normalize_diff_relative_path("b/").is_err());
    }

    #[test]
    fn normalize_diff_rejects_traversal() {
        assert!(normalize_diff_relative_path("../outside.rs").is_err());
    }

    #[test]
    fn normalize_diff_rejects_double_traversal() {
        assert!(normalize_diff_relative_path("a/b/../../outside.rs").is_err());
    }

    #[test]
    fn normalize_diff_rejects_absolute() {
        assert!(normalize_diff_relative_path("/etc/passwd").is_err());
    }

    // --- normalize_request_relative_path tests ---

    fn make_real_path_fixture() -> (tempfile::TempDir, PathBuf, PathBuf) {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("project");
        let file = root.join("src/lib.rs");
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::fs::write(&file, "fn main() {}\n").unwrap();
        (temp, root, file)
    }

    #[test]
    fn normalize_request_absolute_under_root() {
        let (_temp, root, file) = make_real_path_fixture();
        let result = normalize_request_relative_path(&file, &root).unwrap();
        assert_eq!(result, PathBuf::from("src/lib.rs"));
    }

    #[test]
    fn normalize_request_relative_path_resolves() {
        let (_temp, root, _file) = make_real_path_fixture();
        let result = normalize_request_relative_path(Path::new("src/lib.rs"), &root).unwrap();
        assert_eq!(result, PathBuf::from("src/lib.rs"));
    }

    #[test]
    fn normalize_request_rejects_prefix_collision() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("project");
        let file = root.join("src/lib.rs");
        let other = temp.path().join("project-other").join("file.rs");
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::fs::write(&file, "fn main() {}\n").unwrap();
        std::fs::create_dir_all(other.parent().unwrap()).unwrap();
        std::fs::write(&other, "fn other() {}\n").unwrap();
        let result = normalize_request_relative_path(&other, &root);
        assert!(result.is_err(), "prefix collision should be rejected");
    }

    #[test]
    fn normalize_request_rejects_traversal() {
        let (_temp, root, _file) = make_real_path_fixture();
        let result = normalize_request_relative_path(Path::new("../project-other/file.rs"), &root);
        assert!(result.is_err());
    }

    #[test]
    fn normalize_request_rejects_root_itself() {
        let (_temp, root, _file) = make_real_path_fixture();
        let result = normalize_request_relative_path(&root, &root);
        assert!(
            result.is_err(),
            "root itself should be rejected as empty relative path"
        );
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
        let (collector, _temp) = make_collector();
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
        let (collector, _temp) = make_collector();
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
        let (collector, _temp) = make_collector();
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
        let (collector, _temp) = make_collector();
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
        let (collector, _temp) = make_collector();
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

    #[tokio::test]
    async fn collect_rejects_outside_root_path() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("project");
        let file = root.join("src/lib.rs");
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::fs::write(&file, "fn main() {}\n").unwrap();

        let service = Arc::new(LspService::new(LspConfig::default()));
        let operations = Arc::new(LspOperations::new(service.clone()));
        let diagnostics = Arc::new(DiagnosticsCollector::new(service.clone()));
        let sem_collector =
            SemanticContextCollector::new(service, operations, diagnostics, root.clone());
        let nav = HunkSourceNavigator::new();
        let collector = HunkSourceNavigationCollector::new(sem_collector, nav);

        let request = HunkSourceNavigationRequest {
            file_path: "/etc/passwd".to_string(),
            hunks: vec![],
            patch: Some("diff --git a/x b/x\n--- a/x\n+++ b/x\n@@ -1 +1 @@\n-a\n+b\n".to_string()),
            intent: "navigation".to_string(),
            include_definitions: false,
            include_references: false,
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
        let err = result.unwrap_err();
        assert!(
            err.contains("invalid file path") || err.contains("outside"),
            "expected path rejection error: {err}"
        );
    }
}

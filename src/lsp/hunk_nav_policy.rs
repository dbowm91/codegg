use std::path::Path;

/// Configuration for when `hunkSourceContext` should be invoked.
///
/// The policy is conservative by default: definitions and references are
/// on, hierarchy is off, and multi-file / oversized patches are skipped.
#[derive(Debug, Clone)]
pub struct HunkSourceContextPolicy {
    /// Master switch. When false, `hunkSourceContext` is never invoked.
    pub enabled: bool,
    /// Maximum patch size in bytes before the policy skips.
    pub max_patch_bytes: usize,
    /// Maximum number of hunks per file before the policy skips.
    pub max_hunks: usize,
    /// Include definitions in the request.
    pub include_definitions: bool,
    /// Include references in the request.
    pub include_references: bool,
    /// Include call hierarchy in the request.
    pub include_call_hierarchy: bool,
    /// Include type hierarchy in the request.
    pub include_type_hierarchy: bool,
}

impl Default for HunkSourceContextPolicy {
    fn default() -> Self {
        Self {
            enabled: true,
            max_patch_bytes: 64 * 1024,
            max_hunks: 20,
            include_definitions: true,
            include_references: true,
            include_call_hierarchy: false,
            include_type_hierarchy: false,
        }
    }
}

/// The policy decision for whether `hunkSourceContext` should be called.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HunkSourceContextDecision {
    /// Call `hunkSourceContext` with the given file path and patch.
    Use {
        file_path: std::path::PathBuf,
        patch: String,
    },
    /// Skip `hunkSourceContext` for a documented reason.
    Skip { reason: String },
}

/// Known file extensions likely covered by LSP support.
const LSP_LIKELY_EXTENSIONS: &[&str] = &[
    "rs", "py", "ts", "tsx", "js", "jsx", "go", "java", "c", "cpp", "cc", "cxx", "h", "hpp",
    "cs", "rb", "swift", "kt", "kts", "scala", "ex", "exs", "erl", "hs", "ml", "clj",
];

/// Decide whether `hunkSourceContext` should be invoked for a given patch.
///
/// The decision is explicit and testable. Skip reasons are returned as
/// strings for debug logging; they are never silently swallowed.
pub fn decide_hunk_source_context(
    policy: &HunkSourceContextPolicy,
    patch: &str,
    file_path: Option<&Path>,
) -> HunkSourceContextDecision {
    if !policy.enabled {
        return HunkSourceContextDecision::Skip {
            reason: "hunkSourceContext is disabled by policy".to_string(),
        };
    }

    let file_path = match file_path {
        Some(p) => p.to_path_buf(),
        None => {
            return HunkSourceContextDecision::Skip {
                reason: "no file path available".to_string(),
            };
        }
    };

    // Binary/generated file detection via extension.
    if let Some(ext) = file_path.extension().and_then(|e| e.to_str()) {
        let is_lsp_covered = LSP_LIKELY_EXTENSIONS
            .iter()
            .any(|&e| e.eq_ignore_ascii_case(ext));
        if !is_lsp_covered {
            return HunkSourceContextDecision::Skip {
                reason: format!(
                    "file extension .{ext} is unlikely to be covered by an LSP server"
                ),
            };
        }
    } else {
        return HunkSourceContextDecision::Skip {
            reason: "file has no extension (likely binary or generated)".to_string(),
        };
    }

    // Patch size check.
    if patch.len() > policy.max_patch_bytes {
        return HunkSourceContextDecision::Skip {
            reason: format!(
                "patch size {} bytes exceeds cap {} bytes",
                patch.len(),
                policy.max_patch_bytes
            ),
        };
    }

    // Quick hunk count estimate: count @@ lines in the patch.
    let hunk_count = patch.lines().filter(|l| l.starts_with("@@")).count();
    if hunk_count == 0 {
        return HunkSourceContextDecision::Skip {
            reason: "patch contains no @@ hunk headers".to_string(),
        };
    }
    if hunk_count > policy.max_hunks {
        return HunkSourceContextDecision::Skip {
            reason: format!(
                "patch contains {hunk_count} hunks, exceeds cap {}",
                policy.max_hunks
            ),
        };
    }

    HunkSourceContextDecision::Use { file_path, patch: patch.to_string() }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn disabled_policy_skips() {
        let policy = HunkSourceContextPolicy {
            enabled: false,
            ..Default::default()
        };
        let decision =
            decide_hunk_source_context(&policy, "@@ -1 +1 @@\n+new", Some(Path::new("a.rs")));
        assert_eq!(
            decision,
            HunkSourceContextDecision::Skip {
                reason: "hunkSourceContext is disabled by policy".to_string()
            }
        );
    }

    #[test]
    fn no_file_path_skips() {
        let policy = HunkSourceContextPolicy::default();
        let decision = decide_hunk_source_context(&policy, "@@ -1 +1 @@\n+new", None);
        assert_eq!(
            decision,
            HunkSourceContextDecision::Skip {
                reason: "no file path available".to_string()
            }
        );
    }

    #[test]
    fn unsupported_extension_skips() {
        let policy = HunkSourceContextPolicy::default();
        let decision = decide_hunk_source_context(
            &policy,
            "@@ -1 +1 @@\n+new",
            Some(Path::new("image.png")),
        );
        match decision {
            HunkSourceContextDecision::Skip { reason } => {
                assert!(reason.contains(".png"));
            }
            _ => panic!("expected Skip for unsupported extension"),
        }
    }

    #[test]
    fn no_extension_skips() {
        let policy = HunkSourceContextPolicy::default();
        let decision =
            decide_hunk_source_context(&policy, "@@ -1 +1 @@\n+new", Some(Path::new("Makefile")));
        assert_eq!(
            decision,
            HunkSourceContextDecision::Skip {
                reason: "file has no extension (likely binary or generated)".to_string()
            }
        );
    }

    #[test]
    fn oversized_patch_skips() {
        let policy = HunkSourceContextPolicy {
            max_patch_bytes: 10,
            ..Default::default()
        };
        let big_patch = "@@ -1 +1 @@\n+".to_string() + &"x".repeat(20);
        let decision = decide_hunk_source_context(
            &policy,
            &big_patch,
            Some(Path::new("src/main.rs")),
        );
        match decision {
            HunkSourceContextDecision::Skip { reason } => {
                assert!(reason.contains("exceeds cap"));
            }
            _ => panic!("expected Skip for oversized patch"),
        }
    }

    #[test]
    fn no_hunks_skips() {
        let policy = HunkSourceContextPolicy::default();
        let decision =
            decide_hunk_source_context(&policy, "not a diff", Some(Path::new("src/main.rs")));
        assert_eq!(
            decision,
            HunkSourceContextDecision::Skip {
                reason: "patch contains no @@ hunk headers".to_string()
            }
        );
    }

    #[test]
    fn too_many_hunks_skips() {
        let policy = HunkSourceContextPolicy {
            max_hunks: 2,
            ..Default::default()
        };
        let patch = "@@ -1 +1 @@\n+a\n@@ -5 +5 @@\n+b\n@@ -10 +10 @@\n+c";
        let decision =
            decide_hunk_source_context(&policy, patch, Some(Path::new("src/main.rs")));
        match decision {
            HunkSourceContextDecision::Skip { reason } => {
                assert!(reason.contains("exceeds cap"));
            }
            _ => panic!("expected Skip for too many hunks"),
        }
    }

    #[test]
    fn supported_extension_uses() {
        let policy = HunkSourceContextPolicy::default();
        let patch = "@@ -10,6 +10,8 @@\n fn main() {\n+    let x = 1;\n+    let y = 2;\n }";
        let decision =
            decide_hunk_source_context(&policy, patch, Some(Path::new("src/main.rs")));
        assert_eq!(
            decision,
            HunkSourceContextDecision::Use {
                file_path: PathBuf::from("src/main.rs"),
                patch: patch.to_string()
            }
        );
    }

    #[test]
    fn exact_max_hunks_not_skipped() {
        let policy = HunkSourceContextPolicy {
            max_hunks: 2,
            ..Default::default()
        };
        let patch = "@@ -1 +1 @@\n+a\n@@ -5 +5 @@\n+b";
        let decision =
            decide_hunk_source_context(&policy, patch, Some(Path::new("src/main.rs")));
        assert!(matches!(decision, HunkSourceContextDecision::Use { .. }));
    }

    #[test]
    fn exact_max_patch_bytes_not_skipped() {
        let policy = HunkSourceContextPolicy {
            max_patch_bytes: 20,
            ..Default::default()
        };
        let patch = "@@ -1 +1 @@\n+x"; // 14 bytes
        let decision =
            decide_hunk_source_context(&policy, patch, Some(Path::new("src/main.rs")));
        assert!(matches!(decision, HunkSourceContextDecision::Use { .. }));
    }

    #[test]
    fn case_insensitive_extension() {
        let policy = HunkSourceContextPolicy::default();
        let decision = decide_hunk_source_context(
            &policy,
            "@@ -1 +1 @@\n+new",
            Some(Path::new("src/Main.RS")),
        );
        assert!(matches!(decision, HunkSourceContextDecision::Use { .. }));
    }
}

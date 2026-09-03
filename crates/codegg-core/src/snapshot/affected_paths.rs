use std::collections::{BTreeSet, HashSet};

/// Errors from affected-path extraction.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AffectedPathError {
    MissingField(String),
    InvalidMode(String),
    UnsafePath(String),
    UnsupportedTool(String),
    PatchParseFailed(String),
    EmptyPathSet,
}

impl std::fmt::Display for AffectedPathError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AffectedPathError::MissingField(s) => write!(f, "missing field: {}", s),
            AffectedPathError::InvalidMode(s) => write!(f, "invalid mode: {}", s),
            AffectedPathError::UnsafePath(s) => write!(f, "unsafe path: {}", s),
            AffectedPathError::UnsupportedTool(s) => write!(f, "unsupported tool: {}", s),
            AffectedPathError::PatchParseFailed(s) => write!(f, "patch parse failed: {}", s),
            AffectedPathError::EmptyPathSet => write!(f, "empty path set"),
        }
    }
}

impl std::error::Error for AffectedPathError {}

/// Check if a tool is part of the restorable mutation surface.
pub fn is_restorable_tool(name: &str) -> bool {
    matches!(
        name,
        "write" | "edit" | "replace" | "multiedit" | "apply_patch"
    )
}

/// Extract raw affected paths from a single tool call's JSON input.
/// Returns Vec<String> of raw path strings (may be relative or absolute).
/// Caller must normalize and validate.
pub fn extract_affected_paths(
    tool_name: &str,
    input: &serde_json::Value,
) -> Result<Vec<String>, AffectedPathError> {
    match tool_name {
        "write" => {
            let path = input
                .get("path")
                .and_then(|v| v.as_str())
                .ok_or_else(|| AffectedPathError::MissingField("path".into()))?;
            if path.trim().is_empty() {
                return Err(AffectedPathError::MissingField("path empty".into()));
            }
            Ok(vec![path.to_string()])
        }
        "edit" => {
            let path = input
                .get("path")
                .and_then(|v| v.as_str())
                .ok_or_else(|| AffectedPathError::MissingField("path".into()))?;
            if path.trim().is_empty() {
                return Err(AffectedPathError::MissingField("path empty".into()));
            }
            Ok(vec![path.to_string()])
        }
        "replace" => {
            let path = input
                .get("path")
                .and_then(|v| v.as_str())
                .ok_or_else(|| AffectedPathError::MissingField("path".into()))?;
            if path.trim().is_empty() {
                return Err(AffectedPathError::MissingField("path empty".into()));
            }
            Ok(vec![path.to_string()])
        }
        "multiedit" => {
            let path = input
                .get("path")
                .and_then(|v| v.as_str())
                .ok_or_else(|| AffectedPathError::MissingField("path".into()))?;
            if path.trim().is_empty() {
                return Err(AffectedPathError::MissingField("path empty".into()));
            }
            // multiedit applies multiple edits to single path; affected set is one path
            Ok(vec![path.to_string()])
        }
        "apply_patch" => {
            let mode = input
                .get("mode")
                .and_then(|v| v.as_str())
                .unwrap_or("update");
            match mode {
                "update" | "create" | "delete" => {
                    let path = input
                        .get("path")
                        .and_then(|v| v.as_str())
                        .ok_or_else(|| AffectedPathError::MissingField("path".into()))?;
                    if path.trim().is_empty() {
                        return Err(AffectedPathError::MissingField("path empty".into()));
                    }
                    Ok(vec![path.to_string()])
                }
                "move" => {
                    let patch = input
                        .get("patch")
                        .and_then(|v| v.as_str())
                        .ok_or_else(|| AffectedPathError::MissingField("patch".into()))?;
                    if let Some((old, new)) = parse_move_paths(patch) {
                        if old.trim().is_empty() || new.trim().is_empty() {
                            return Err(AffectedPathError::PatchParseFailed(
                                "move paths empty".into(),
                            ));
                        }
                        Ok(vec![old, new])
                    } else {
                        Err(AffectedPathError::PatchParseFailed(
                            "move requires rename header".into(),
                        ))
                    }
                }
                other => Err(AffectedPathError::InvalidMode(other.to_string())),
            }
        }
        other => Err(AffectedPathError::UnsupportedTool(other.to_string())),
    }
}

/// Extract and normalize affected paths for an entire batch.
/// Returns None if batch contains no restorable tools.
/// Returns Err if any restorable tool has ambiguous/unsafe derivation.
pub fn extract_batch_affected_paths(
    tool_calls: &[(String, serde_json::Value)],
) -> Result<Option<Vec<String>>, AffectedPathError> {
    extract_batch_affected_paths_with_read_only(tool_calls, |_, _| false)
}

/// Extract paths for a logical batch while allowing callers to provide the
/// authoritative classification for tools that are affirmatively read-only.
///
/// A batch containing a supported restorable mutation and an unknown or
/// potentially mutating call is non-restorable as a whole. This preserves the
/// provenance boundary: a native path subset must not be checkpointed while an
/// untracked call may have changed one of those paths.
pub fn extract_batch_affected_paths_with_read_only(
    tool_calls: &[(String, serde_json::Value)],
    is_read_only: impl Fn(&str, &serde_json::Value) -> bool,
) -> Result<Option<Vec<String>>, AffectedPathError> {
    let mut all = Vec::new();
    let mut has_restorable = false;
    let mut has_unknown_side_effect = false;
    for (name, input) in tool_calls {
        if is_restorable_tool(name) {
            has_restorable = true;
            let paths = extract_affected_paths(name, input)?;
            for p in paths {
                // Basic unsafe check: reject empty or absolute with traversal?
                // Absolute paths are allowed but will be normalized later against workspace root.
                // Here we reject paths containing NUL or empty.
                if p.contains('\0') {
                    return Err(AffectedPathError::UnsafePath(p));
                }
                all.push(p);
            }
        } else if !is_read_only(name, input) {
            has_unknown_side_effect = true;
        }
    }
    if !has_restorable || has_unknown_side_effect {
        return Ok(None);
    }
    if all.is_empty() {
        return Err(AffectedPathError::EmptyPathSet);
    }
    Ok(Some(all))
}

/// Normalize paths to workspace-relative form and deduplicate.
/// Returns sorted deduped list. Validates safe relative paths after normalization.
/// Absolute paths are made relative if they are under project_root.
pub fn normalize_and_dedup(
    raw_paths: Vec<String>,
    project_root: &std::path::Path,
) -> Result<Vec<String>, AffectedPathError> {
    let mut set = BTreeSet::new();
    for raw in raw_paths {
        let normalized = normalize_single_path(&raw, project_root)?;
        // Validate safe relative after normalization
        let pb = std::path::PathBuf::from(&normalized);
        if pb.is_absolute() {
            return Err(AffectedPathError::UnsafePath(normalized));
        }
        if !crate::snapshot::is_safe_relative_path(&pb) {
            return Err(AffectedPathError::UnsafePath(normalized));
        }
        if normalized.is_empty() {
            return Err(AffectedPathError::UnsafePath("empty".into()));
        }
        set.insert(normalized);
    }
    Ok(set.into_iter().collect())
}

fn normalize_single_path(
    raw: &str,
    project_root: &std::path::Path,
) -> Result<String, AffectedPathError> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err(AffectedPathError::UnsafePath("empty path".into()));
    }
    let p = std::path::Path::new(trimmed);
    if p.is_absolute() {
        // Try to make relative to project_root
        match p.strip_prefix(project_root) {
            Ok(rel) => {
                let s = rel.to_string_lossy().to_string();
                if s.is_empty() {
                    Err(AffectedPathError::UnsafePath(trimmed.to_string()))
                } else {
                    Ok(s)
                }
            }
            Err(_) => {
                // Check if project_root is canonicalized? Try with string prefix
                Err(AffectedPathError::UnsafePath(trimmed.to_string()))
            }
        }
    } else {
        // Keep as is, but clean leading ./ if present
        let s = trimmed.strip_prefix("./").unwrap_or(trimmed).to_string();
        Ok(s)
    }
}

/// Parse move old/new paths from patch content.
/// Supports both `rename from/rename to` and `--- a/ / +++ b/` headers.
fn parse_move_paths(patch: &str) -> Option<(String, String)> {
    for line in patch.lines() {
        if line.starts_with("rename from ") {
            let old = line.strip_prefix("rename from ")?;
            for line2 in patch.lines() {
                if line2.starts_with("rename to ") {
                    let new = line2.strip_prefix("rename to ")?;
                    return Some((old.to_string(), new.to_string()));
                }
            }
        }
    }
    for line in patch.lines() {
        if let Some(rest) = line.strip_prefix("--- a/") {
            for line2 in patch.lines() {
                if let Some(new) = line2.strip_prefix("+++ b/") {
                    return Some((rest.to_string(), new.to_string()));
                }
            }
        }
    }
    None
}

/// Detect overlapping paths within a normalized deduped set?
/// Overlap in this context means duplicate raw paths that were deduped.
/// Caller can compare raw len vs deduped len to infer overlap.
pub fn has_overlapping_paths(raw_len: usize, deduped_len: usize) -> bool {
    raw_len != deduped_len
}

/// Check if two batches overlap (any common path).
pub fn batches_overlap(a: &[String], b: &[String]) -> bool {
    let set_a: HashSet<_> = a.iter().collect();
    for p in b {
        if set_a.contains(p) {
            return true;
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn write_extracts_one_path() {
        let input = json!({"path":"src/main.rs","content":"hi"});
        assert_eq!(
            extract_affected_paths("write", &input).unwrap(),
            vec!["src/main.rs"]
        );
    }

    #[test]
    fn edit_extracts_one_path() {
        let input = json!({"path":"a.txt","old_string":"x","new_string":"y"});
        assert_eq!(
            extract_affected_paths("edit", &input).unwrap(),
            vec!["a.txt"]
        );
    }

    #[test]
    fn replace_extracts_one_path() {
        let input = json!({"path":"a.txt","pattern":"x","replacement":"y"});
        assert_eq!(
            extract_affected_paths("replace", &input).unwrap(),
            vec!["a.txt"]
        );
    }

    #[test]
    fn multiedit_extracts_one_path() {
        let input = json!({"path":"a.txt","edits":[{"old_string":"x","new_string":"y"}]});
        assert_eq!(
            extract_affected_paths("multiedit", &input).unwrap(),
            vec!["a.txt"]
        );
    }

    #[test]
    fn apply_patch_update_extracts_path() {
        let input = json!({"path":"a.txt","patch":"@@","mode":"update"});
        assert_eq!(
            extract_affected_paths("apply_patch", &input).unwrap(),
            vec!["a.txt"]
        );
    }
    #[test]
    fn apply_patch_create_extracts_path() {
        let input = json!({"path":"new.txt","patch":"content","mode":"create"});
        assert_eq!(
            extract_affected_paths("apply_patch", &input).unwrap(),
            vec!["new.txt"]
        );
    }
    #[test]
    fn apply_patch_delete_extracts_path() {
        let input = json!({"path":"old.txt","patch":"","mode":"delete"});
        assert_eq!(
            extract_affected_paths("apply_patch", &input).unwrap(),
            vec!["old.txt"]
        );
    }
    #[test]
    fn apply_patch_move_extracts_both() {
        let patch = "rename from old.txt\nrename to new.txt\n";
        let input = json!({"path":"ignored","patch":patch,"mode":"move"});
        let paths = extract_affected_paths("apply_patch", &input).unwrap();
        assert_eq!(paths, vec!["old.txt", "new.txt"]);
    }
    #[test]
    fn apply_patch_move_alternate_header() {
        let patch = "--- a/old.txt\n+++ b/new.txt\n@@ -1 +1 @@\n-old\n+new\n";
        let input = json!({"path":"old.txt","patch":patch,"mode":"move"});
        let paths = extract_affected_paths("apply_patch", &input).unwrap();
        assert_eq!(paths, vec!["old.txt", "new.txt"]);
    }
    #[test]
    fn move_missing_patch_fails() {
        let input = json!({"path":"x","mode":"move"});
        assert!(matches!(
            extract_affected_paths("apply_patch", &input),
            Err(AffectedPathError::MissingField(_))
        ));
    }
    #[test]
    fn normalize_dedup() {
        let root = std::path::Path::new("/tmp/ws");
        let raw = vec!["a.txt".into(), "a.txt".into(), "./b.txt".into()];
        let normalized = normalize_and_dedup(raw, root).unwrap();
        assert_eq!(normalized, vec!["a.txt", "b.txt"]);
    }
    #[test]
    fn normalize_rejects_traversal() {
        let root = std::path::Path::new("/tmp/ws");
        let raw = vec!["../evil.txt".into()];
        assert!(matches!(
            normalize_and_dedup(raw, root),
            Err(AffectedPathError::UnsafePath(_))
        ));
    }
    #[test]
    fn batch_extraction_none_when_no_restorable() {
        let calls = vec![("bash".to_string(), json!({"command":"ls"}))];
        assert_eq!(extract_batch_affected_paths(&calls).unwrap(), None);
    }
    #[test]
    fn batch_extraction_aggregates() {
        let calls = vec![
            ("write".to_string(), json!({"path":"a.txt","content":"hi"})),
            (
                "edit".to_string(),
                json!({"path":"b.txt","old_string":"x","new_string":"y"}),
            ),
        ];
        let opt = extract_batch_affected_paths(&calls).unwrap().unwrap();
        assert_eq!(opt.len(), 2);
        assert!(opt.contains(&"a.txt".to_string()));
        assert!(opt.contains(&"b.txt".to_string()));
    }
    #[test]
    fn malformed_move_cannot_produce_incomplete_checkpoint() {
        // missing rename header should error, not produce partial
        let input = json!({"path":"a.txt","patch":"no headers","mode":"move"});
        assert!(extract_affected_paths("apply_patch", &input).is_err());
    }

    #[test]
    fn mixed_unknown_side_effect_is_not_restorable() {
        let calls = vec![
            (
                "write".to_string(),
                json!({"path": "a.txt", "content": "new"}),
            ),
            ("bash".to_string(), json!({"command": "touch a.txt"})),
        ];
        assert_eq!(
            extract_batch_affected_paths_with_read_only(&calls, |_, _| false).unwrap(),
            None
        );
    }

    #[test]
    fn mixed_affirmative_read_only_call_remains_restorable() {
        let calls = vec![
            (
                "write".to_string(),
                json!({"path": "a.txt", "content": "new"}),
            ),
            ("read".to_string(), json!({"path": "a.txt"})),
        ];
        let paths = extract_batch_affected_paths_with_read_only(&calls, |name, _| name == "read")
            .unwrap()
            .unwrap();
        assert_eq!(paths, vec!["a.txt"]);
    }
}

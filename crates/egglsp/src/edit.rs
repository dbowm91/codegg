use std::path::{Path, PathBuf};

use lsp_types::{
    DocumentChangeOperation, DocumentChanges, OneOf, TextEdit, Uri,
    WorkspaceEdit as LspWorkspaceEdit,
};
use sha2::{Digest, Sha256};
use similar::TextDiff;
use url::Url;

use crate::error::LspError;

const MAX_EDIT_PREVIEW_FILES: usize = 20;
const MAX_EDIT_PREVIEW_EDITS: usize = 1000;
const MAX_REPLACEMENT_PREVIEW_CHARS: usize = 500;
const MAX_PATCH_CHARS_PER_FILE: usize = 50_000;

#[derive(Debug, Clone, serde::Serialize)]
pub struct WorkspaceEditPreview {
    pub title: String,
    pub files: Vec<FileEditPreview>,
    pub total_files: usize,
    pub total_edits: usize,
    pub truncated: bool,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct FileEditPreview {
    pub file: PathBuf,
    pub original_hash: String,
    pub edits: Vec<TextEditPreview>,
    pub patch: String,
    pub patch_omitted: bool,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct TextEditPreview {
    pub start_line: u32,
    pub start_column: u32,
    pub end_line: u32,
    pub end_column: u32,
    pub replacement_preview: String,
}

pub fn preview_workspace_edit(
    title: impl Into<String>,
    edit: LspWorkspaceEdit,
    allowed_root: Option<&Path>,
) -> Result<WorkspaceEditPreview, LspError> {
    let title = title.into();
    let mut files: Vec<FileEditPreview> = Vec::new();
    let mut total_files: usize = 0;
    let mut total_edits: usize = 0;
    let mut truncated = false;

    if let Some(changes) = edit.changes {
        for (uri, text_edits) in changes {
            total_files += 1;
            total_edits += text_edits.len();
            if files.len() >= MAX_EDIT_PREVIEW_FILES {
                truncated = true;
                continue;
            }
            let path = uri_to_path(&uri)?;
            validate_path_against_root(&path, allowed_root)?;
            match build_file_preview(&path, text_edits, allowed_root) {
                Ok(fp) => {
                    if fp.patch_omitted {
                        truncated = true;
                    }
                    files.push(fp);
                }
                Err(e) => return Err(e),
            }
        }
    }

    if let Some(doc_changes) = edit.document_changes {
        match doc_changes {
            DocumentChanges::Edits(edits) => {
                for tde in edits {
                    let path = uri_to_path(&tde.text_document.uri)?;
                    validate_path_against_root(&path, allowed_root)?;
                    let text_edits: Vec<TextEdit> = tde
                        .edits
                        .into_iter()
                        .map(|one| match one {
                            OneOf::Left(te) => te,
                            OneOf::Right(ate) => ate.text_edit,
                        })
                        .collect();
                    total_files += 1;
                    total_edits += text_edits.len();
                    if files.len() >= MAX_EDIT_PREVIEW_FILES {
                        truncated = true;
                        continue;
                    }
                    match build_file_preview(&path, text_edits, allowed_root) {
                        Ok(fp) => {
                            if fp.patch_omitted {
                                truncated = true;
                            }
                            files.push(fp);
                        }
                        Err(e) => return Err(e),
                    }
                }
            }
            DocumentChanges::Operations(ops) => {
                for op in ops {
                    match op {
                        DocumentChangeOperation::Edit(tde) => {
                            let path = uri_to_path(&tde.text_document.uri)?;
                            validate_path_against_root(&path, allowed_root)?;
                            let text_edits: Vec<TextEdit> = tde
                                .edits
                                .into_iter()
                                .map(|one| match one {
                                    OneOf::Left(te) => te,
                                    OneOf::Right(ate) => ate.text_edit,
                                })
                                .collect();
                            total_files += 1;
                            total_edits += text_edits.len();
                            if files.len() >= MAX_EDIT_PREVIEW_FILES {
                                truncated = true;
                                continue;
                            }
                            match build_file_preview(&path, text_edits, allowed_root) {
                                Ok(fp) => {
                                    if fp.patch_omitted {
                                        truncated = true;
                                    }
                                    files.push(fp);
                                }
                                Err(e) => return Err(e),
                            }
                        }
                        DocumentChangeOperation::Op(res) => {
                            return Err(LspError::UnsupportedEdit(format!(
                                "resource operations (create/rename/delete) are not supported in preview: {:?}",
                                res
                            )));
                        }
                    }
                }
            }
        }
    }

    if total_files > MAX_EDIT_PREVIEW_FILES || total_edits > MAX_EDIT_PREVIEW_EDITS {
        truncated = true;
    }

    Ok(WorkspaceEditPreview {
        title,
        files,
        total_files,
        total_edits,
        truncated,
    })
}

pub fn preview_text_edits_for_file(
    title: impl Into<String>,
    file_path: &Path,
    edits: Vec<TextEdit>,
    allowed_root: Option<&Path>,
) -> Result<WorkspaceEditPreview, LspError> {
    let title = title.into();
    let logical_edits = edits.len();
    let mut truncated = logical_edits > MAX_EDIT_PREVIEW_EDITS;

    validate_path_against_root(file_path, allowed_root)?;
    let fp = build_file_preview(file_path, edits, allowed_root)?;
    if fp.patch_omitted {
        truncated = true;
    }
    let total_files = 1;
    let total_edits = logical_edits;
    if total_edits > MAX_EDIT_PREVIEW_EDITS {
        truncated = true;
    }

    Ok(WorkspaceEditPreview {
        title,
        files: vec![fp],
        total_files,
        total_edits,
        truncated,
    })
}

fn uri_to_path(uri: &Uri) -> Result<PathBuf, LspError> {
    let s = uri.as_str();
    let url = Url::parse(s)
        .map_err(|e| LspError::RequestFailed(format!("invalid file uri '{}': {}", s, e)))?;
    url.to_file_path()
        .map_err(|_| LspError::RequestFailed(format!("uri is not a file path: {}", s)))
}

fn validate_path_against_root(path: &Path, allowed_root: Option<&Path>) -> Result<(), LspError> {
    if let Some(root) = allowed_root {
        let root_canon = root.canonicalize().map_err(LspError::from)?;
        let p = if path.is_absolute() {
            path.to_path_buf()
        } else {
            std::env::current_dir()
                .unwrap_or_else(|_| PathBuf::from("."))
                .join(path)
        };
        let check_path = if p.exists() {
            p.canonicalize().unwrap_or(p)
        } else if let Some(parent) = p.parent() {
            if parent.exists() {
                parent
                    .canonicalize()
                    .map(|cp| cp.join(p.file_name().unwrap_or_default()))
                    .unwrap_or(p)
            } else {
                p
            }
        } else {
            p
        };
        if !check_path.starts_with(&root_canon) {
            return Err(LspError::PathOutsideRoot(path.display().to_string()));
        }
    }
    Ok(())
}

fn make_relative_path(path: &Path, allowed_root: Option<&Path>) -> String {
    if let Some(root) = allowed_root {
        if let Ok(stripped) = path.strip_prefix(root) {
            return stripped.to_string_lossy().into_owned();
        }
        if let (Ok(cp), Ok(cr)) = (path.canonicalize(), root.canonicalize()) {
            if let Ok(s) = cp.strip_prefix(cr) {
                return s.to_string_lossy().into_owned();
            }
        }
    }
    if let Ok(cwd) = std::env::current_dir() {
        if let Ok(s) = path.strip_prefix(&cwd) {
            return s.to_string_lossy().into_owned();
        }
        if let (Ok(cp), Ok(cc)) = (path.canonicalize(), cwd.canonicalize()) {
            if let Ok(s) = cp.strip_prefix(cc) {
                return s.to_string_lossy().into_owned();
            }
        }
    }
    path.to_string_lossy().into_owned()
}

fn build_file_preview(
    file_path: &Path,
    edits: Vec<TextEdit>,
    allowed_root: Option<&Path>,
) -> Result<FileEditPreview, LspError> {
    let original = std::fs::read_to_string(file_path)?;
    let original_hash = {
        let digest = Sha256::digest(original.as_bytes());
        format!("{:x}", digest)
    };

    let new_content = apply_text_edits(&original, &edits)?;

    let mut preview_edits: Vec<TextEditPreview> = Vec::new();
    for te in &edits {
        if preview_edits.len() >= 100 {
            break;
        }
        let repl = te.new_text.clone();
        let replacement_preview = if repl.len() > MAX_REPLACEMENT_PREVIEW_CHARS {
            let mut p: String = repl.chars().take(MAX_REPLACEMENT_PREVIEW_CHARS).collect();
            p.push_str("...");
            p
        } else {
            repl
        };
        preview_edits.push(TextEditPreview {
            start_line: te.range.start.line,
            start_column: te.range.start.character,
            end_line: te.range.end.line,
            end_column: te.range.end.character,
            replacement_preview,
        });
    }

    let rel = make_relative_path(file_path, allowed_root);
    let mut patch = generate_unified_patch(&original, &new_content, &rel);
    let patch_omitted = patch.len() > MAX_PATCH_CHARS_PER_FILE;
    if patch_omitted {
        patch = String::new();
    }

    Ok(FileEditPreview {
        file: file_path.to_path_buf(),
        original_hash,
        edits: preview_edits,
        patch,
        patch_omitted,
    })
}

fn generate_unified_patch(old: &str, new: &str, rel_path: &str) -> String {
    if old == new {
        return "(no changes)\n".to_string();
    }
    let diff = TextDiff::from_lines(old, new);
    let mut result = String::new();
    result.push_str(&format!("--- a/{}\n", rel_path));
    result.push_str(&format!("+++ b/{}\n", rel_path));
    let groups = diff.grouped_ops(3);
    let mut has_hunk = false;
    for group in &groups {
        if group.is_empty() {
            continue;
        }
        has_hunk = true;
        let mut old_start: Option<usize> = None;
        let mut new_start: Option<usize> = None;
        let mut old_cnt = 0usize;
        let mut new_cnt = 0usize;
        for op in group {
            for ch in diff.iter_changes(op) {
                if ch.tag() != similar::ChangeTag::Insert {
                    if old_start.is_none() {
                        old_start = ch.old_index();
                    }
                    old_cnt += 1;
                }
                if ch.tag() != similar::ChangeTag::Delete {
                    if new_start.is_none() {
                        new_start = ch.new_index();
                    }
                    new_cnt += 1;
                }
            }
        }
        let os = old_start.unwrap_or(0) + 1;
        let ns = new_start.unwrap_or(0) + 1;
        result.push_str(&format!("@@ -{},{} +{},{} @@\n", os, old_cnt, ns, new_cnt));
        for op in group {
            for change in diff.iter_changes(op) {
                let sign = match change.tag() {
                    similar::ChangeTag::Delete => "-",
                    similar::ChangeTag::Insert => "+",
                    similar::ChangeTag::Equal => " ",
                };
                let val = change.value().trim_end_matches(['\n', '\r']);
                result.push_str(&format!("{}{}\n", sign, val));
            }
        }
    }
    if !has_hunk {
        result.push_str("(no changes)\n");
    }
    result
}

fn line_start_offsets(text: &str) -> Vec<usize> {
    let mut offsets = vec![0];
    for (i, c) in text.char_indices() {
        if c == '\n' {
            offsets.push(i + 1);
        }
    }
    offsets
}

fn utf16_position_to_byte_offset(text: &str, line: u32, character: u32) -> Result<usize, LspError> {
    let starts = line_start_offsets(text);
    let li = line as usize;
    if li >= starts.len() {
        return Err(LspError::Utf16Position(format!(
            "line {} out of range ({} lines)",
            line,
            starts.len().saturating_sub(1)
        )));
    }
    let line_start_byte = starts[li];
    let line_content_end = if li + 1 < starts.len() {
        let nl_pos = starts[li + 1] - 1;
        if nl_pos > line_start_byte && text.as_bytes().get(nl_pos.saturating_sub(1)) == Some(&b'\r')
        {
            nl_pos - 1
        } else {
            nl_pos
        }
    } else {
        text.len()
    };
    if line_content_end < line_start_byte {
        if character == 0 {
            return Ok(line_start_byte);
        }
        return Err(LspError::Utf16Position(format!(
            "character {} out of range for empty line {}",
            character, line
        )));
    }
    let line_content = &text[line_start_byte..line_content_end];
    let mut utf16_idx: u32 = 0;
    let mut byte_off = line_start_byte;
    for c in line_content.chars() {
        let cu = c.len_utf16() as u32;
        if utf16_idx == character {
            return Ok(byte_off);
        }
        if utf16_idx + cu > character {
            return Err(LspError::Utf16Position(format!(
                "utf16 offset {} splits surrogate pair on line {}",
                character, line
            )));
        }
        utf16_idx += cu;
        byte_off += c.len_utf8();
    }
    if utf16_idx == character {
        return Ok(byte_off);
    }
    Err(LspError::Utf16Position(format!(
        "character {} out of range for line {} ({} utf16 units)",
        character, line, utf16_idx
    )))
}

fn apply_text_edits(text: &str, edits: &[TextEdit]) -> Result<String, LspError> {
    if edits.is_empty() {
        return Ok(text.to_string());
    }
    let mut ops: Vec<(usize, usize, String)> = Vec::new();
    for e in edits {
        let start =
            utf16_position_to_byte_offset(text, e.range.start.line, e.range.start.character)?;
        let end = utf16_position_to_byte_offset(text, e.range.end.line, e.range.end.character)?;
        if end < start {
            return Err(LspError::Utf16Position("range end before start".into()));
        }
        ops.push((start, end, e.new_text.clone()));
    }
    let mut asc = ops.clone();
    asc.sort_by_key(|t| t.0);
    for i in 1..asc.len() {
        if asc[i].0 < asc[i - 1].1 {
            return Err(LspError::OverlappingEdits);
        }
    }
    ops.sort_by_key(|b| std::cmp::Reverse(b.0));
    let mut result = text.to_string();
    for (start, end, repl) in ops {
        if start > result.len() || end > result.len() {
            return Err(LspError::Utf16Position(
                "edit range exceeds current content during apply".into(),
            ));
        }
        result.replace_range(start..end, &repl);
    }
    Ok(result)
}

#[cfg(test)]
#[allow(dead_code)]
fn generate_unified_patch_for_test(old: &str, new: &str, rel_path: &str) -> String {
    generate_unified_patch(old, new, rel_path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use lsp_types::{Position, Range, TextEdit};

    #[test]
    fn apply_single_line_edit() {
        let text = "hello world\nsecond line\n";
        let edits = vec![TextEdit {
            range: Range {
                start: Position {
                    line: 0,
                    character: 6,
                },
                end: Position {
                    line: 0,
                    character: 11,
                },
            },
            new_text: "rust".to_string(),
        }];
        let res = apply_text_edits(text, &edits).unwrap();
        assert_eq!(res, "hello rust\nsecond line\n");
    }

    #[test]
    fn apply_multiline_edit() {
        let text = "line1\nline2\nline3\n";
        let edits = vec![TextEdit {
            range: Range {
                start: Position {
                    line: 0,
                    character: 0,
                },
                end: Position {
                    line: 2,
                    character: 0,
                },
            },
            new_text: "NEW\nCONTENT\n".to_string(),
        }];
        let res = apply_text_edits(text, &edits).unwrap();
        assert_eq!(res, "NEW\nCONTENT\nline3\n");
    }

    #[test]
    fn apply_insert_at_start() {
        let text = "existing";
        let edits = vec![TextEdit {
            range: Range {
                start: Position {
                    line: 0,
                    character: 0,
                },
                end: Position {
                    line: 0,
                    character: 0,
                },
            },
            new_text: "PRE-".to_string(),
        }];
        let res = apply_text_edits(text, &edits).unwrap();
        assert_eq!(res, "PRE-existing");
    }

    #[test]
    fn apply_insert_at_end() {
        let text = "start\nend";
        let edits = vec![TextEdit {
            range: Range {
                start: Position {
                    line: 1,
                    character: 3,
                },
                end: Position {
                    line: 1,
                    character: 3,
                },
            },
            new_text: "TAIL".to_string(),
        }];
        let res = apply_text_edits(text, &edits).unwrap();
        assert_eq!(res, "start\nendTAIL");
    }

    #[test]
    fn apply_unicode_utf16_position() {
        let text = "hi \u{1F600} there\n";
        let edits = vec![TextEdit {
            range: Range {
                start: Position {
                    line: 0,
                    character: 3,
                },
                end: Position {
                    line: 0,
                    character: 5,
                },
            },
            new_text: "X".to_string(),
        }];
        let res = apply_text_edits(text, &edits).unwrap();
        assert_eq!(res, "hi X there\n");
    }

    #[test]
    fn reject_out_of_bounds_edit() {
        let text = "short";
        let edits = vec![TextEdit {
            range: Range {
                start: Position {
                    line: 0,
                    character: 0,
                },
                end: Position {
                    line: 0,
                    character: 100,
                },
            },
            new_text: "x".to_string(),
        }];
        let err = apply_text_edits(text, &edits).unwrap_err();
        assert!(matches!(err, LspError::Utf16Position(_)));
    }

    #[test]
    fn reject_overlapping_edits() {
        let text = "abcdef";
        let edits = vec![
            TextEdit {
                range: Range {
                    start: Position {
                        line: 0,
                        character: 0,
                    },
                    end: Position {
                        line: 0,
                        character: 4,
                    },
                },
                new_text: "X".to_string(),
            },
            TextEdit {
                range: Range {
                    start: Position {
                        line: 0,
                        character: 2,
                    },
                    end: Position {
                        line: 0,
                        character: 5,
                    },
                },
                new_text: "Y".to_string(),
            },
        ];
        let err = apply_text_edits(text, &edits).unwrap_err();
        assert!(matches!(err, LspError::OverlappingEdits));
    }

    #[test]
    fn apply_multiple_edits_reverse_order() {
        let text = "0123456789";
        let edits = vec![
            TextEdit {
                range: Range {
                    start: Position {
                        line: 0,
                        character: 0,
                    },
                    end: Position {
                        line: 0,
                        character: 1,
                    },
                },
                new_text: "A".to_string(),
            },
            TextEdit {
                range: Range {
                    start: Position {
                        line: 0,
                        character: 8,
                    },
                    end: Position {
                        line: 0,
                        character: 9,
                    },
                },
                new_text: "B".to_string(),
            },
        ];
        let res = apply_text_edits(text, &edits).unwrap();
        assert_eq!(res, "A1234567B9");
    }

    #[test]
    fn patch_contains_file_headers() {
        let old = "a\nb\nc\n";
        let new = "a\nB\nc\n";
        let p = generate_unified_patch_for_test(old, new, "src/foo.rs");
        assert!(p.contains("--- a/src/foo.rs"));
        assert!(p.contains("+++ b/src/foo.rs"));
    }

    #[test]
    fn patch_contains_hunk() {
        let old = "fn foo() {}\n";
        let new = "fn foo() { bar(); }\n";
        let p = generate_unified_patch_for_test(old, new, "bar.rs");
        assert!(p.contains("@@ -1,1 +1,1 @@"));
        assert!(p.contains("-fn foo() {}"));
        assert!(p.contains("+fn foo() { bar(); }"));
    }

    #[test]
    fn patch_omitted_or_errors_when_over_cap() {
        let old = "x".repeat(10);
        let new = "y".repeat(60000);
        let p = generate_unified_patch_for_test(&old, &new, "big.txt");
        assert!(p.len() > MAX_PATCH_CHARS_PER_FILE || p.contains("omitted"));
        let tmp = std::env::temp_dir().join("egglsp_test_big.txt");
        std::fs::write(&tmp, &old).unwrap();
        let title = "big";
        let te = TextEdit {
            range: Range {
                start: Position {
                    line: 0,
                    character: 0,
                },
                end: Position {
                    line: 0,
                    character: 1,
                },
            },
            new_text: "y".repeat(60000),
        };
        let preview = preview_text_edits_for_file(title, &tmp, vec![te], None);
        let wp = preview.unwrap();
        assert!(wp.truncated);
        assert!(wp.files[0].patch_omitted);
        let _ = std::fs::remove_file(&tmp);
    }

    #[test]
    fn format_preview_rejects_path_outside_allowed_root() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("allowed");
        std::fs::create_dir_all(&root).unwrap();
        let outside = Path::new("/etc/passwd");
        let edits = vec![TextEdit {
            range: Range {
                start: Position {
                    line: 0,
                    character: 0,
                },
                end: Position {
                    line: 0,
                    character: 0,
                },
            },
            new_text: "x".to_string(),
        }];
        let err = preview_text_edits_for_file("format", outside, edits, Some(&root)).unwrap_err();
        assert!(matches!(err, LspError::PathOutsideRoot(_)));
    }

    #[test]
    fn large_patch_sets_patch_omitted_field() {
        let dir = tempfile::tempdir().unwrap();
        let file_path = dir.path().join("big.txt");
        let original = "x".repeat(10);
        std::fs::write(&file_path, &original).unwrap();
        let te = TextEdit {
            range: Range {
                start: Position {
                    line: 0,
                    character: 0,
                },
                end: Position {
                    line: 0,
                    character: 1,
                },
            },
            new_text: "y".repeat(60000),
        };
        let preview = preview_text_edits_for_file("big", &file_path, vec![te], None).unwrap();
        assert!(preview.files[0].patch_omitted);
        assert!(preview.truncated);
        assert!(preview.files[0].patch.is_empty());
    }

    #[test]
    fn workspace_truncated_uses_structured_flag_not_patch_string() {
        let dir = tempfile::tempdir().unwrap();
        let file_path = dir.path().join("big.txt");
        let original = "x".repeat(10);
        std::fs::write(&file_path, &original).unwrap();
        let te = TextEdit {
            range: Range {
                start: Position {
                    line: 0,
                    character: 0,
                },
                end: Position {
                    line: 0,
                    character: 1,
                },
            },
            new_text: "y".repeat(60000),
        };
        let preview = preview_text_edits_for_file("big", &file_path, vec![te], None).unwrap();
        assert!(!preview.files[0].patch.contains("omitted"));
        assert!(preview.files[0].patch_omitted);
    }

    #[test]
    fn workspace_edit_preview_type_is_reexported() {
        let _ = std::any::type_name::<crate::WorkspaceEditPreview>();
        let _ = std::any::type_name::<crate::FileEditPreview>();
        let _ = std::any::type_name::<crate::TextEditPreview>();
    }
}

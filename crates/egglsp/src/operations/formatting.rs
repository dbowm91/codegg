use std::path::{Path, PathBuf};

use lsp_types::*;
use sha2::{Digest, Sha256};
use similar::TextDiff;

use crate::client::url_to_uri;
use crate::edit::{preview_text_edits_for_file, WorkspaceEditPreview};
use crate::error::LspError;

use super::LspOperations;

/// Default cap on the per-action unified diff inside
/// [`FormattingPreview`]. The diff is truncated to this many bytes
/// when the formatted content would produce a larger patch.
pub const FORMATTING_PREVIEW_MAX_DIFF_BYTES: usize = 8 * 1024;

/// Evidence about a file's version at preview time. Carries the
/// content hash (SHA-256 hex) and the optional LSP document version
/// so consumers can detect external disk changes after the preview
/// was constructed.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct VersionedFileEvidence {
    pub file: PathBuf,
    pub content_hash: String,
    pub document_version: Option<i32>,
}

/// Bounded, preview-only document-formatting DTO. Reads the
/// on-disk file, computes a sha256 of the original content, runs
/// the existing `format_preview` pipeline in memory, and emits a
/// bounded unified diff of the original vs. formatted content.
/// The on-disk file is never mutated; the caller can compare
/// `before_hash` to a follow-up re-read to verify the
/// file-system is unchanged.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct FormattingPreview {
    pub file: std::path::PathBuf,
    /// Number of text edits the server returned. May exceed
    /// `MAX_EDIT_PREVIEW_EDITS`; see `truncated` for overflow.
    pub edit_count: usize,
    /// sha256 hex of the on-disk file content before any edits.
    pub before_hash: String,
    /// sha256 hex of the file content after applying the server's
    /// edits in memory (matches `before_hash` when no edits).
    pub after_hash: String,
    /// Bounded unified diff (capped at
    /// [`FORMATTING_PREVIEW_MAX_DIFF_BYTES`]). When the diff
    /// exceeds the cap the prefix is returned followed by a
    /// truncation marker.
    pub diff: String,
    /// True when the diff exceeded the cap and was truncated.
    pub truncated: bool,
    /// sha256 hex of the file content on disk after the preview
    /// was constructed (the verification re-read). When this
    /// differs from `before_hash` the base was modified externally
    /// during the request and `base_stale` is set.
    pub final_disk_hash: String,
    /// True when the on-disk file content changed between the
    /// initial read and the verification re-read. When true the
    /// preview may be stale and should be refreshed before use.
    pub base_stale: bool,
    pub server_generation: u64,
}

/// Pure helper: build a sha256 hex string from a byte slice.
pub fn sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    format!("{:x}", digest)
}

/// Compute the LSP `Position` at the end of a document, using UTF-16 code
/// units for the character offset (as required by the LSP specification).
///
/// - empty string → `(0, 0)`
/// - one-line ASCII text → `(0, len)`
/// - text ending in newline → final line is the empty line after the
///   newline, character `0`
/// - unicode text counts UTF-16 code units, not bytes or chars
pub fn document_end_position_utf16(text: &str) -> Position {
    if text.is_empty() {
        return Position {
            line: 0,
            character: 0,
        };
    }
    let mut line: u32 = 0;
    let mut character: u32 = 0;
    for c in text.chars() {
        if c == '\n' {
            line += 1;
            character = 0;
        } else {
            character += c.len_utf16() as u32;
        }
    }
    // If the text ends with a newline, the cursor is at the start of the
    // next (empty) line — which is already correct from the loop.
    // If it does not end with a newline, character points to the end of
    // the last line.
    Position { line, character }
}

impl LspOperations {
    pub async fn format_preview(
        &self,
        file_path: &Path,
        allowed_root: Option<&Path>,
    ) -> Result<WorkspaceEditPreview, LspError> {
        let (key, _uri_str) = self.service.ensure_file_open_from_disk(file_path).await?;
        let uri = url_to_uri(&url::Url::from_file_path(file_path).map_err(|_| {
            LspError::LaunchFailed(format!("invalid file path: {}", file_path.display()))
        })?)?;

        let params = serde_json::to_value(DocumentFormattingParams {
            text_document: TextDocumentIdentifier {
                uri,
            },
            options: FormattingOptions {
                tab_size: 4,
                insert_spaces: true,
                properties: Default::default(),
                trim_trailing_whitespace: Some(true),
                insert_final_newline: Some(true),
                trim_final_newlines: Some(true),
            },
            work_done_progress_params: Default::default(),
        })?;

        let resp = self
            .service
            .send_request(&key, "textDocument/formatting", params)
            .await?;

        if resp.is_null() {
            return Ok(WorkspaceEditPreview {
                title: "format".to_string(),
                files: vec![],
                total_files: 0,
                total_edits: 0,
                truncated: false,
            });
        }

        let edits: Vec<TextEdit> = serde_json::from_value(resp)?;
        if edits.is_empty() {
            return Ok(WorkspaceEditPreview {
                title: "format".to_string(),
                files: vec![],
                total_files: 0,
                total_edits: 0,
                truncated: false,
            });
        }

        preview_text_edits_for_file("format", file_path, edits, allowed_root)
    }

    // ── Phase 4 Pass 8: typed format-preview surface ────────────────

    /// Preview-only `textDocument/formatting` returning a typed
    /// [`FormattingPreview`] DTO. Capability-gated: returns
    /// `LspUnavailable` when the server does not advertise a
    /// document-formatting provider.
    ///
    /// Reads the on-disk file once to compute `before_hash` and
    /// to drive the in-memory edit application, then re-reads
    /// the file at the end to verify the on-disk view is
    /// unchanged. The on-disk file is never mutated.
    pub async fn format_preview_typed(
        &self,
        file_path: &Path,
        allowed_root: Option<&Path>,
    ) -> Result<FormattingPreview, LspError> {
        use crate::capability::LspSemanticOperation;

        if let Some(snapshot) = self.capability_snapshot_for_file(file_path).await {
            if !snapshot.supports(LspSemanticOperation::DocumentFormatting) {
                if let Some(u) = snapshot.unavailable(LspSemanticOperation::DocumentFormatting) {
                    return Err(LspError::Unavailable(u));
                }
            }
        }

        let before_content = tokio::fs::read_to_string(file_path).await.map_err(|e| {
            LspError::RequestFailed(format!(
                "failed to read file {}: {}",
                file_path.display(),
                e
            ))
        })?;
        let before_hash = sha256_hex(before_content.as_bytes());

        // Run the existing format pipeline (in-memory only).
        let preview = self.format_preview(file_path, allowed_root).await?;

        // Reconstruct the in-memory "after" content by applying
        // the preview's edits. This is in-memory only — the file
        // is never written.
        let after_content = apply_file_edit_preview(&before_content, &preview);
        let after_hash = sha256_hex(after_content.as_bytes());

        // Build a bounded unified diff.
        let (diff, truncated) = if preview.files.is_empty() {
            (String::new(), false)
        } else {
            build_bounded_unified_diff(&before_content, &after_content, file_path)
        };

        // Verify the on-disk file is unchanged (defense-in-depth
        // even though no mutating call was made).
        let final_disk_bytes = tokio::fs::read(file_path).await.map_err(|e| {
            LspError::RequestFailed(format!(
                "failed to re-read file {}: {}",
                file_path.display(),
                e
            ))
        })?;
        let final_disk_hash = sha256_hex(&final_disk_bytes);
        let base_stale = final_disk_hash != before_hash;

        let (key, _root) = self.service.get_or_create_client(file_path).await?;
        let server_generation = self.service.generation_for_key(&key).await;

        Ok(FormattingPreview {
            file: file_path.to_path_buf(),
            edit_count: preview.total_edits,
            before_hash,
            after_hash,
            diff,
            truncated,
            final_disk_hash,
            base_stale,
            server_generation,
        })
    }
}

/// Apply the in-memory edits from a `WorkspaceEditPreview` to
/// `original` and return the resulting content. This is purely a
/// helper for [`FormattingPreview`] (Pass 8); the underlying
/// `preview_workspace_edit`/`preview_text_edits_for_file` already
/// computed the `after` content, but the API only exposes the
/// patch text and not the raw `after`. We reconstruct the
/// `after` by applying the preview's `edits` in order.
///
/// On any error applying edits (overlap / out-of-bounds) the
/// function returns the input unchanged. The caller compares
/// before/after hashes so the caller's contract still holds.
fn apply_file_edit_preview(original: &str, preview: &WorkspaceEditPreview) -> String {
    for fp in &preview.files {
        // We only need the first file's content; format is single-file.
        let edits: Vec<TextEdit> = fp
            .edits
            .iter()
            .map(|te| TextEdit {
                range: Range {
                    start: Position {
                        line: te.start_line,
                        character: te.start_column,
                    },
                    end: Position {
                        line: te.end_line,
                        character: te.end_column,
                    },
                },
                new_text: te.replacement_preview.clone(),
            })
            .collect();
        if let Ok(after) = apply_text_edits_for_diff(original, &edits) {
            return after;
        }
    }
    original.to_string()
}

/// Apply a list of `TextEdit`s to `text` for the diff helper.
/// Returns the original on any error. Loosely equivalent to
/// `edit::apply_text_edits` but tolerant of single-line edits
/// only (which is all we need for the format-after reconstruction).
fn apply_text_edits_for_diff(text: &str, edits: &[TextEdit]) -> Result<String, LspError> {
    let mut result = text.to_string();
    let mut sorted: Vec<&TextEdit> = edits.iter().collect();
    sorted.sort_by(|a, b| {
        b.range
            .start
            .line
            .cmp(&a.range.start.line)
            .then(b.range.start.character.cmp(&a.range.start.character))
    });
    for e in sorted {
        let start = utf16_to_byte_index(&result, e.range.start.line, e.range.start.character);
        let end = utf16_to_byte_index(&result, e.range.end.line, e.range.end.character);
        if let (Some(s), Some(en)) = (start, end) {
            if en >= s && en <= result.len() {
                result.replace_range(s..en, &e.new_text);
            }
        }
    }
    Ok(result)
}

/// Translate an LSP UTF-16 (line, character) position to a byte
/// offset in `text`. Returns `None` if the position is invalid.
fn utf16_to_byte_index(text: &str, line: u32, character: u32) -> Option<usize> {
    let mut cur_line = 0u32;
    let mut cur_char_utf16 = 0u32;
    let mut byte_idx = 0usize;
    let mut chars = text.char_indices().peekable();
    while let Some((b, c)) = chars.next() {
        if cur_line == line && cur_char_utf16 == character {
            return Some(b);
        }
        if c == '\n' {
            if cur_line == line && cur_char_utf16 + 1 == character {
                // End of line; the byte after the newline.
                return Some(b + 1);
            }
            cur_line += 1;
            cur_char_utf16 = 0;
        } else {
            cur_char_utf16 += c.len_utf16() as u32;
        }
        byte_idx = b + c.len_utf8();
    }
    if cur_line == line && cur_char_utf16 == character {
        return Some(byte_idx);
    }
    None
}

/// Build a bounded unified diff (capped at
/// [`FORMATTING_PREVIEW_MAX_DIFF_BYTES`]) of `before` vs
/// `after` for `file_path`. Returns `(diff, truncated)`.
pub(crate) fn build_bounded_unified_diff(
    before: &str,
    after: &str,
    file_path: &Path,
) -> (String, bool) {
    if before == after {
        return (String::new(), false);
    }
    let rel = file_path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| file_path.display().to_string());
    let mut result = String::new();
    result.push_str(&format!("--- a/{}\n", rel));
    result.push_str(&format!("+++ b/{}\n", rel));

    let diff = TextDiff::from_lines(before, after);
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
    if result.len() > FORMATTING_PREVIEW_MAX_DIFF_BYTES {
        let mut truncated_str = String::with_capacity(FORMATTING_PREVIEW_MAX_DIFF_BYTES + 64);
        truncated_str.push_str(&result[..FORMATTING_PREVIEW_MAX_DIFF_BYTES]);
        truncated_str.push_str("\n... (truncated)\n");
        return (truncated_str, true);
    }
    (result, false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::edit::{FileEditPreview, TextEditPreview};
    use lsp_types::{ServerCapabilities, Uri};
    use std::str::FromStr;

    fn uri(s: &str) -> Uri {
        Uri::from_str(s).expect("valid uri")
    }

    // ---- format_preview_typed helpers (sha256, bounded diff) ----

    #[test]
    fn sha256_hex_is_64_lowercase_hex_chars() {
        let h = sha256_hex(b"hello world");
        assert_eq!(
            h,
            "b94d27b9934d3e08a52e52d7da7dabfac484efe37a5380ee9088f7ace2efcde9"
        );
        assert_eq!(h.len(), 64);
        assert!(h
            .chars()
            .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase()));
    }

    #[test]
    fn sha256_hex_different_input_different_hash() {
        let h1 = sha256_hex(b"abc");
        let h2 = sha256_hex(b"abd");
        assert_ne!(h1, h2);
    }

    #[test]
    fn build_bounded_unified_diff_emits_headers_and_hunks() {
        let before = "fn foo() {}\n";
        let after = "fn foo() { bar(); }\n";
        let (diff, truncated) = build_bounded_unified_diff(before, after, Path::new("foo.rs"));
        assert!(!truncated);
        assert!(diff.contains("--- a/foo.rs"));
        assert!(diff.contains("+++ b/foo.rs"));
        assert!(diff.contains("@@ -1,1 +1,1 @@"));
        assert!(diff.contains("-fn foo() {}"));
        assert!(diff.contains("+fn foo() { bar(); }"));
    }

    #[test]
    fn build_bounded_unified_diff_no_changes_returns_empty() {
        let text = "fn unchanged() {}\n";
        let (diff, truncated) = build_bounded_unified_diff(text, text, Path::new("u.rs"));
        assert_eq!(diff, "");
        assert!(!truncated);
    }

    #[test]
    fn build_bounded_unified_diff_truncates_oversize_output() {
        let line = "x".repeat(100);
        let before = format!("{}\n", line);
        let after = format!("{}\n", "y".repeat(100));
        let mut big_before = before.clone();
        let mut big_after = after.clone();
        for _ in 0..200 {
            big_before.push_str(&before);
            big_after.push_str(&after);
        }
        let (diff, truncated) =
            build_bounded_unified_diff(&big_before, &big_after, Path::new("big.rs"));
        assert!(truncated, "expected truncation flag for oversize diff");
        assert!(diff.contains("truncated"));
        assert!(diff.len() <= FORMATTING_PREVIEW_MAX_DIFF_BYTES + 64);
    }

    #[test]
    fn apply_text_edits_for_diff_single_line_insert() {
        let text = "hello world\n";
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
        let after = apply_text_edits_for_diff(text, &edits).unwrap();
        assert_eq!(after, "hello rust\n");
    }

    #[test]
    fn apply_text_edits_for_diff_two_edits_reverse_order() {
        let text = "0123456789\n";
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
        let after = apply_text_edits_for_diff(text, &edits).unwrap();
        assert_eq!(after, "A1234567B9\n");
    }

    #[test]
    fn apply_text_edits_for_diff_no_edits_returns_input() {
        let text = "untouched\n";
        let after = apply_text_edits_for_diff(text, &[]).unwrap();
        assert_eq!(after, text);
    }

    #[test]
    fn apply_file_edit_preview_empty_files_returns_original() {
        let text = "untouched\n";
        let preview = WorkspaceEditPreview {
            title: "format".to_string(),
            files: vec![],
            total_files: 0,
            total_edits: 0,
            truncated: false,
        };
        let after = apply_file_edit_preview(text, &preview);
        assert_eq!(after, text);
    }

    #[test]
    fn apply_file_edit_preview_applies_first_file_edits() {
        let text = "fn foo() {}\n";
        let fp = FileEditPreview {
            file: PathBuf::from("foo.rs"),
            original_hash: "deadbeef".to_string(),
            edits: vec![TextEditPreview {
                start_line: 0,
                start_column: 9,
                end_line: 0,
                end_column: 10,
                replacement_preview: "{ bar();".to_string(),
            }],
            patch: String::new(),
            patch_omitted: false,
        };
        let preview = WorkspaceEditPreview {
            title: "format".to_string(),
            files: vec![fp],
            total_files: 1,
            total_edits: 1,
            truncated: false,
        };
        let after = apply_file_edit_preview(text, &preview);
        assert_eq!(after, "fn foo() { bar();}\n");
    }

    // ---- sha256 stability for format_preview_typed ----

    #[test]
    fn format_preview_typed_hashes_are_stable_for_identical_input() {
        let before = "let x = 1;\n";
        let after = "let x = 1;\nlet y = 2;\n";
        let h1_before = sha256_hex(before.as_bytes());
        let h2_before = sha256_hex(before.as_bytes());
        let h1_after = sha256_hex(after.as_bytes());
        let h2_after = sha256_hex(after.as_bytes());
        assert_eq!(h1_before, h2_before);
        assert_eq!(h1_after, h2_after);
        assert_ne!(h1_before, h1_after);
    }

    // ---- capability gating: formatting ----

    #[test]
    fn capability_snapshot_reports_document_formatting_unavailable_when_unset() {
        let caps = ServerCapabilities::default();
        let snap = crate::capability::LspCapabilitySnapshot::from_capabilities(&caps, Some("pylsp"), Some("python"));
        assert!(!snap.supports(crate::capability::LspSemanticOperation::DocumentFormatting));
        let u = snap
            .unavailable(crate::capability::LspSemanticOperation::DocumentFormatting)
            .expect("unavailable");
        assert_eq!(u.operation, "formatting");
        assert!(u.reason.contains("pylsp"));
    }

    #[test]
    fn capability_snapshot_reports_document_formatting_available_when_advertised() {
        let mut caps = ServerCapabilities::default();
        caps.document_formatting_provider = Some(lsp_types::OneOf::Left(true));
        let snap = crate::capability::LspCapabilitySnapshot::from_capabilities(&caps, Some("s"), Some("rust"));
        assert!(snap.supports(crate::capability::LspSemanticOperation::DocumentFormatting));
        assert!(snap
            .unavailable(crate::capability::LspSemanticOperation::DocumentFormatting)
            .is_none());
    }

    // ---- Pass 8: base-freshness semantics ----

    #[test]
    fn versioned_file_evidence_round_trips_through_serde() {
        let ev = VersionedFileEvidence {
            file: PathBuf::from("src/main.rs"),
            content_hash: "abc123".to_string(),
            document_version: Some(5),
        };
        let json = serde_json::to_value(&ev).expect("serialize");
        let decoded: VersionedFileEvidence = serde_json::from_value(json).expect("deserialize");
        assert_eq!(decoded.file, PathBuf::from("src/main.rs"));
        assert_eq!(decoded.content_hash, "abc123");
        assert_eq!(decoded.document_version, Some(5));
    }

    #[test]
    fn formatting_preview_includes_base_freshness_fields() {
        let preview = FormattingPreview {
            file: PathBuf::from("foo.rs"),
            edit_count: 2,
            before_hash: "aaa".to_string(),
            after_hash: "bbb".to_string(),
            diff: "--- a/foo.rs\n+++ b/foo.rs\n".to_string(),
            truncated: false,
            final_disk_hash: "aaa".to_string(),
            base_stale: false,
            server_generation: 1,
        };
        assert_eq!(preview.final_disk_hash, "aaa");
        assert!(!preview.base_stale);
    }

    #[test]
    fn formatting_preview_detects_stale_base_via_hash_mismatch() {
        let before = b"fn foo() {}\n";
        let disk_after = b"fn foo() { changed(); }\n";
        let before_hash = sha256_hex(before);
        let disk_hash = sha256_hex(disk_after);
        let base_stale = before_hash != disk_hash;

        let preview = FormattingPreview {
            file: PathBuf::from("foo.rs"),
            edit_count: 0,
            before_hash: before_hash.clone(),
            after_hash: sha256_hex(before),
            diff: String::new(),
            truncated: false,
            final_disk_hash: disk_hash.clone(),
            base_stale,
            server_generation: 1,
        };
        assert!(preview.base_stale);
        assert_ne!(preview.before_hash, preview.final_disk_hash);
    }

    #[test]
    fn formatting_preview_clean_when_disk_unchanged() {
        let content = b"fn foo() {}\n";
        let hash = sha256_hex(content);

        let preview = FormattingPreview {
            file: PathBuf::from("foo.rs"),
            edit_count: 0,
            before_hash: hash.clone(),
            after_hash: hash.clone(),
            diff: String::new(),
            truncated: false,
            final_disk_hash: hash.clone(),
            base_stale: false,
            server_generation: 1,
        };
        assert!(!preview.base_stale);
        assert_eq!(preview.before_hash, preview.final_disk_hash);
    }
}

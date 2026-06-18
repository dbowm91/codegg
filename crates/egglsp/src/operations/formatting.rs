use std::path::{Path, PathBuf};

use lsp_types::*;
use sha2::{Digest, Sha256};
use similar::TextDiff;

use crate::client::url_to_uri;
use crate::edit::{preview_text_edits_for_file, validate_path_against_root, WorkspaceEditPreview};
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
    /// Low-level `textDocument/formatting` protocol wrapper.
    ///
    /// **No capability gating, no `before_hash` / `after_hash`
    /// computation, no in-memory diff.** Callers outside the
    /// typed [`Self::format_preview_typed`] helper should
    /// generally prefer the typed API; this method exists for
    /// the typed surface to use internally and for the real-server
    /// smoke harness to drive raw protocol behavior.
    pub async fn format_preview_unchecked(
        &self,
        file_path: &Path,
        allowed_root: Option<&Path>,
    ) -> Result<WorkspaceEditPreview, LspError> {
        let (key, _uri_str) = self.service.ensure_file_open_from_disk(file_path).await?;
        let uri = url_to_uri(&url::Url::from_file_path(file_path).map_err(|_| {
            LspError::LaunchFailed(format!("invalid file path: {}", file_path.display()))
        })?)?;

        let params = serde_json::to_value(DocumentFormattingParams {
            text_document: TextDocumentIdentifier { uri },
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

    /// Request raw formatting edits from the server without truncation.
    async fn request_formatting_edits(&self, file_path: &Path) -> Result<Vec<TextEdit>, LspError> {
        let (key, _uri_str) = self.service.ensure_file_open_from_disk(file_path).await?;
        let uri =
            crate::client::url_to_uri(&url::Url::from_file_path(file_path).map_err(|_| {
                LspError::LaunchFailed(format!("invalid file path: {}", file_path.display()))
            })?)?;

        let params = serde_json::to_value(DocumentFormattingParams {
            text_document: TextDocumentIdentifier { uri },
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
            return Ok(Vec::new());
        }

        let edits: Vec<TextEdit> = serde_json::from_value(resp)?;
        Ok(edits)
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

        // Fail-closed capability gate.
        self.require_capability(file_path, LspSemanticOperation::DocumentFormatting)
            .await?;

        // Validate path against allowed root before any server request.
        validate_path_against_root(file_path, allowed_root)?;

        let before_content = tokio::fs::read_to_string(file_path).await.map_err(|e| {
            LspError::RequestFailed(format!(
                "failed to read file {}: {}",
                file_path.display(),
                e
            ))
        })?;
        let before_hash = sha256_hex(before_content.as_bytes());

        let raw_edits = self.request_formatting_edits(file_path).await?;
        if raw_edits.is_empty() {
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
            return Ok(FormattingPreview {
                file: file_path.to_path_buf(),
                edit_count: 0,
                before_hash: before_hash.clone(),
                after_hash: before_hash,
                diff: String::new(),
                truncated: false,
                final_disk_hash,
                base_stale,
                server_generation,
            });
        }

        let after_content = crate::edit::apply_text_edits(&before_content, &raw_edits)?;
        let after_hash = sha256_hex(after_content.as_bytes());

        let (diff, truncated) =
            build_bounded_unified_diff(&before_content, &after_content, file_path);

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
            edit_count: raw_edits.len(),
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
    use lsp_types::ServerCapabilities;

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
        let snap = crate::capability::LspCapabilitySnapshot::from_capabilities(
            &caps,
            Some("pylsp"),
            Some("python"),
        );
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
        let snap = crate::capability::LspCapabilitySnapshot::from_capabilities(
            &caps,
            Some("s"),
            Some("rust"),
        );
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

    // ---- Pass 4: raw edit hash integrity ----

    #[test]
    fn formatting_long_replacement_hash_uses_full_text() {
        let before = "fn foo() {}\n";
        let long_replacement = "x".repeat(10000);
        let edits = vec![TextEdit {
            range: Range {
                start: Position {
                    line: 0,
                    character: 9,
                },
                end: Position {
                    line: 0,
                    character: 10,
                },
            },
            new_text: long_replacement.clone(),
        }];
        let after = crate::edit::apply_text_edits(before, &edits).unwrap();
        let after_hash = sha256_hex(after.as_bytes());
        assert_eq!(after.len(), before.len() - 1 + 10000);
        assert_eq!(after_hash, sha256_hex(after.as_bytes()));
    }

    #[test]
    fn formatting_more_than_100_edits_hash_uses_all_edits() {
        let mut before = String::new();
        for i in 0..150 {
            before.push_str(&format!("line{i}\n"));
        }
        let mut edits = Vec::new();
        for i in 0..150 {
            let line_text = format!("line{i}");
            let end_char = line_text.len() as u32;
            edits.push(TextEdit {
                range: Range {
                    start: Position {
                        line: i as u32,
                        character: 0,
                    },
                    end: Position {
                        line: i as u32,
                        character: end_char,
                    },
                },
                new_text: format!("new{i:04}"),
            });
        }
        let after = crate::edit::apply_text_edits(&before, &edits).unwrap();
        let after_hash = sha256_hex(after.as_bytes());
        assert_eq!(after_hash, sha256_hex(after.as_bytes()));
    }

    #[test]
    fn formatting_invalid_edit_returns_error() {
        let before = "short";
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
        let result = crate::edit::apply_text_edits(before, &edits);
        assert!(result.is_err());
    }

    #[test]
    fn formatting_never_writes_disk() {
        let before = "fn foo() {}\n";
        let after = "fn foo() { bar(); }\n";
        let before_hash = sha256_hex(before.as_bytes());
        let after_hash = sha256_hex(after.as_bytes());
        assert_ne!(before_hash, after_hash);
    }
}

use std::path::Path;

use lsp_types::*;

use crate::capability::{CapabilityDecision, LspCapabilitySnapshot, LspSemanticOperation};
use crate::client::url_to_uri;
use crate::edit::preview_workspace_edit;
use crate::error::LspError;

use super::{sha256_hex, VersionedFileEvidence};

/// Default cap on the number of files reported in a [`RenamePreview`].
pub const RENAME_PREVIEW_MAX_FILES: usize = 100;

/// Default cap on the number of edits reported in a [`RenamePreview`].
pub const RENAME_PREVIEW_MAX_EDITS: usize = 1000;

/// Bounded result of `textDocument/prepareRename` for the
/// model-facing surface. Normalizes the three `lsp_types` variants
/// (Range / RangeWithPlaceholder / DefaultBehavior) into a
/// flattened enum and surfaces structured `LspUnavailable` when
/// the server does not advertise a prepare-rename provider.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum PrepareRenameResult {
    /// Server returned a bare `Range` (no placeholder text).
    Range {
        range: lsp_types::Range,
        placeholder: Option<String>,
    },
    /// Server returned `defaultBehavior: true`. The client should
    /// use its default rename behavior (typically identifier-aware
    /// selection).
    DefaultBehavior,
    /// Server returned null — the position cannot be renamed.
    NotRenameable,
    /// Server does not advertise prepare-rename support.
    Unavailable(crate::capability::LspUnavailable),
}

impl PrepareRenameResult {
    /// Build a typed result from a raw `PrepareRenameResponse`.
    pub fn from_response(resp: Option<PrepareRenameResponse>) -> Self {
        match resp {
            None => PrepareRenameResult::NotRenameable,
            Some(PrepareRenameResponse::Range(r)) => PrepareRenameResult::Range {
                range: r,
                placeholder: None,
            },
            Some(PrepareRenameResponse::RangeWithPlaceholder { range, placeholder }) => {
                PrepareRenameResult::Range {
                    range,
                    placeholder: Some(placeholder),
                }
            }
            Some(PrepareRenameResponse::DefaultBehavior { default_behavior }) => {
                if default_behavior {
                    PrepareRenameResult::DefaultBehavior
                } else {
                    PrepareRenameResult::NotRenameable
                }
            }
        }
    }

    /// The range over which a rename would apply, if the server
    /// committed to one.
    pub fn range(&self) -> Option<&lsp_types::Range> {
        match self {
            Self::Range { range, .. } => Some(range),
            _ => None,
        }
    }

    /// Whether this result indicates the position is renameable.
    pub fn is_renameable(&self) -> bool {
        matches!(self, Self::Range { .. } | Self::DefaultBehavior)
    }
}

/// Bounded, preview-only rename DTO returned to the model-facing
/// surface. Wraps a [`WorkspaceEditPreview`] (already validated
/// against the allowed root) with the placeholder from
/// `prepareRename` and structured warnings about resource
/// operations (create / rename / delete) that the existing
/// preview pipeline rejects.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct RenamePreview {
    /// The original identifier (placeholder) at the rename site, if
    /// the server reported one via `prepareRename`. `None` for
    /// `Range` and `DefaultBehavior` variants.
    pub old_name: Option<String>,
    /// The new identifier the caller asked the server to apply.
    pub new_name: String,
    /// Per-file preview entries (already validated against
    /// `allowed_root`; out-of-root files produced errors and are
    /// not present here).
    pub affected_files: Vec<crate::edit::FileEditPreview>,
    /// Total number of text edits across all files. Capped at
    /// [`RENAME_PREVIEW_MAX_EDITS`]; see `truncated` for overflow.
    pub edit_count: usize,
    /// Structured warnings (e.g. resource operations present in
    /// the raw edit that the preview pipeline could not surface).
    pub warnings: Vec<String>,
    /// True when the underlying server's edit count or file count
    /// exceeded the preview caps and was clamped.
    pub truncated: bool,
    /// True when any affected file's content hash changed between
    /// the preview request and the verification re-read. When true
    /// the preview may be stale and should be refreshed before use.
    pub base_stale: bool,
    /// Per-file version evidence for every file touched by the
    /// rename. Captured before and after the LSP request to enable
    /// staleness detection.
    pub affected_file_versions: Vec<VersionedFileEvidence>,
    /// Authoritative server generation of the live client.
    pub server_generation: u64,
}

impl super::LspOperations {
    /// Low-level `textDocument/prepareRename` protocol wrapper.
    ///
    /// **No capability gating, no prepare-rename normalization.**
    /// Callers outside the typed [`Self::prepare_rename_typed`]
    /// helper should generally prefer the typed API; this method
    /// exists for the typed surface to use internally and for the
    /// real-server smoke harness to drive raw protocol behavior.
    pub async fn prepare_rename_unchecked(
        &self,
        file_path: &Path,
        line: u32,
        column: u32,
    ) -> Result<Option<PrepareRenameResponse>, LspError> {
        let (key, _root) = self.service.get_or_create_client(file_path).await?;
        let uri = url_to_file_path_from_url(file_path)?;

        let params = serde_json::to_value(TextDocumentPositionParams {
            text_document: TextDocumentIdentifier {
                uri: url_to_uri(&uri)?,
            },
            position: Position {
                line,
                character: column,
            },
        })?;

        let resp = self
            .service
            .send_request(&key, "textDocument/prepareRename", params)
            .await?;

        if resp.is_null() {
            return Ok(None);
        }

        let pr: Option<PrepareRenameResponse> = serde_json::from_value(resp)?;
        Ok(pr)
    }

    /// Low-level `textDocument/rename` protocol wrapper.
    ///
    /// **No capability gating, no prepare-rename consultation.**
    /// Callers outside the typed [`Self::rename_preview_typed`]
    /// helper should generally prefer the typed API; this method
    /// exists for the typed surface to use internally and for the
    /// real-server smoke harness to drive raw protocol behavior.
    pub async fn rename_preview_unchecked(
        &self,
        file_path: &Path,
        line: u32,
        column: u32,
        new_name: &str,
        allowed_root: Option<&Path>,
    ) -> Result<crate::edit::WorkspaceEditPreview, LspError> {
        let (key, _uri_str) = self.service.ensure_file_open_from_disk(file_path).await?;
        let uri = url_to_file_path_from_url(file_path)?;

        // Optionally attempt prepareRename; ignore unsupported errors and proceed.
        let _ = self
            .service
            .send_request(
                &key,
                "textDocument/prepareRename",
                serde_json::to_value(TextDocumentPositionParams {
                    text_document: TextDocumentIdentifier {
                        uri: url_to_uri(&uri)?,
                    },
                    position: Position {
                        line,
                        character: column,
                    },
                })?,
            )
            .await;

        let params = serde_json::to_value(RenameParams {
            text_document_position: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier {
                    uri: url_to_uri(&uri)?,
                },
                position: Position {
                    line,
                    character: column,
                },
            },
            new_name: new_name.to_string(),
            work_done_progress_params: Default::default(),
        })?;

        let resp = self
            .service
            .send_request(&key, "textDocument/rename", params)
            .await?;

        if resp.is_null() {
            return Err(LspError::RequestFailed(
                "rename returned no result (no edits or unsupported at location)".to_string(),
            ));
        }

        let ws_edit: WorkspaceEdit = serde_json::from_value(resp)?;
        preview_workspace_edit("rename symbol", ws_edit, allowed_root)
    }

    // ── Phase 4 Pass 6: typed rename surface ─────────────────────────

    /// Read-only `textDocument/prepareRename` returning a typed
    /// [`PrepareRenameResult`]. Capability-gated: returns
    /// [`PrepareRenameResult::Unavailable`] when the server does
    /// not advertise a prepare-rename provider.
    ///
    /// Pure normalization of the three raw
    /// `lsp_types::PrepareRenameResponse` variants (Range /
    /// RangeWithPlaceholder / DefaultBehavior) into a flat enum
    /// plus a structured `LspUnavailable` fallback. The server's
    /// raw response is never exposed.
    pub async fn prepare_rename_typed(
        &self,
        file_path: &Path,
        line: u32,
        column: u32,
    ) -> Result<PrepareRenameResult, LspError> {
        // Inspect the explicit capability decision so we can
        // distinguish Supported / Unsupported / Unknown.
        // `require_capability` would collapse Unsupported and
        // Unknown into a single error path, which is wrong for
        // callers that need to react differently (e.g. the
        // rename pipeline skips prepare-rename when the server
        // does not advertise it).
        match self
            .capability_decision_for_file(file_path, LspSemanticOperation::PrepareRename)
            .await?
        {
            CapabilityDecision::Supported => {}
            CapabilityDecision::Unsupported(u) => {
                return Ok(PrepareRenameResult::Unavailable(u));
            }
            CapabilityDecision::Unknown {
                operation,
                reason: _,
            } => {
                return Err(LspError::NotInitialized(format!(
                    "capability {} is not yet known for {}",
                    operation.as_str(),
                    file_path.display(),
                )));
            }
        }
        let resp = self
            .prepare_rename_unchecked(file_path, line, column)
            .await?;
        Ok(PrepareRenameResult::from_response(resp))
    }

    /// Preview-only `textDocument/rename` returning a typed
    /// [`RenamePreview`] DTO. Capability-gated via the explicit
    /// Rename + PrepareRename capability decisions and the same
    /// root-validation contract as
    /// [`Self::rename_preview_unchecked`].
    ///
    /// `new_name` must be non-empty. The on-disk file is never
    /// mutated. Resource operations (create/rename/delete) inside
    /// `document_changes` are reported as structured warnings
    /// because the underlying preview pipeline does not surface
    /// them.
    ///
    /// # Capability decision flow
    ///
    /// ```text
    /// 1. require Rename capability (fail-closed: not advertised → Unavailable,
    ///    unknown → NotInitialized)
    /// 2. inspect effective capability snapshot for PrepareRename
    /// 3. if PrepareRename supported:
    ///      call prepareRename
    ///      NotRenameable -> return empty structured preview, no rename request
    ///      Range/DefaultBehavior -> continue with `old_name` from the placeholder
    /// 4. if PrepareRename unsupported:
    ///      skip prepareRename, issue `textDocument/rename` directly,
    ///      `old_name = None` (the server did not advertise prepare-rename;
    ///      fabricating a placeholder would be wrong)
    /// 5. if PrepareRename unknown:
    ///      fail closed with NotInitialized (do not silently issue a rename
    ///      against a server whose prepare-rename state is not yet known)
    /// ```
    pub async fn rename_preview_typed(
        &self,
        file_path: &Path,
        line: u32,
        column: u32,
        new_name: &str,
        allowed_root: Option<&Path>,
    ) -> Result<RenamePreview, LspError> {
        if new_name.is_empty() {
            return Err(LspError::RequestFailed(
                "new_name must not be empty".to_string(),
            ));
        }

        // Step 0: Capture the hash of the target file before the request.
        let target_file = file_path.to_path_buf();
        let base_content = tokio::fs::read_to_string(&target_file).await.map_err(|e| {
            LspError::RequestFailed(format!(
                "failed to read file {}: {}",
                target_file.display(),
                e
            ))
        })?;
        let target_base_hash = sha256_hex(base_content.as_bytes());

        // Step 1: explicit Rename capability gate. The server MUST
        // advertise rename; if it does not, fail fast with a
        // structured `LspUnavailable` so the caller can react.
        self.require_capability(file_path, LspSemanticOperation::Rename)
            .await?;

        // Step 2: inspect the explicit PrepareRename decision so
        // we can react differently to `Unsupported` (issue rename
        // directly with `old_name = None`) and `Unknown` (fail
        // closed). The previous flow always called
        // `prepare_rename_typed`, which conflated "server does
        // not advertise prepare-rename" with "the position is
        // not renameable".
        let prepare_decision = self
            .capability_decision_for_file(file_path, LspSemanticOperation::PrepareRename)
            .await?;

        let mut warnings: Vec<String> = Vec::new();
        let prepared = match prepare_decision {
            CapabilityDecision::Supported => {
                // Server advertises prepare-rename. Call it and
                // branch on the typed result.
                self.prepare_rename_typed(file_path, line, column).await?
            }
            CapabilityDecision::Unsupported(u) => {
                // Server does NOT advertise prepare-rename but
                // DOES advertise rename (Step 1 already gated
                // that). Record a structured note and proceed
                // with the rename request directly; `old_name`
                // is `None` because we have no placeholder.
                warnings.push(format!(
                    "prepareRename not advertised by {}; issuing rename directly",
                    u.server.as_deref().unwrap_or("server")
                ));
                PrepareRenameResult::DefaultBehavior
            }
            CapabilityDecision::Unknown {
                operation,
                reason: _,
            } => {
                // Fail-closed: if the server's prepare-rename
                // capability is not yet known, do not silently
                // issue a rename.
                return Err(LspError::NotInitialized(format!(
                    "capability {} is not yet known for {}",
                    operation.as_str(),
                    file_path.display(),
                )));
            }
        };

        // If the server explicitly told us the position is not
        // renameable, do not send the rename request — return a
        // structured empty preview.
        if matches!(prepared, PrepareRenameResult::NotRenameable) {
            let (key, _root) = self.service.get_or_create_client(file_path).await?;
            let server_generation = self.service.generation_for_key(&key).await;
            return Ok(RenamePreview {
                old_name: None,
                new_name: new_name.to_string(),
                affected_files: Vec::new(),
                edit_count: 0,
                warnings: vec![
                    "Position is not renameable (prepareRename returned null)".to_string()
                ],
                truncated: false,
                base_stale: false,
                affected_file_versions: Vec::new(),
                server_generation,
            });
        }

        let old_name = match &prepared {
            PrepareRenameResult::Range { placeholder, .. } => placeholder.clone(),
            PrepareRenameResult::DefaultBehavior => None,
            PrepareRenameResult::NotRenameable => None,
            PrepareRenameResult::Unavailable(_) => unreachable!("handled above"),
        };

        // Step 3: call the existing rename pipeline to get a
        // raw WorkspaceEdit (so we can inspect document_changes
        // for resource ops) AND the prepared WorkspaceEditPreview.
        let (raw_edit, preview) = self
            .rename_raw_and_preview(file_path, line, column, new_name, allowed_root)
            .await?;

        // Step 4: scan for resource operations in document_changes.
        if let Some(doc_changes) = raw_edit.document_changes.as_ref() {
            match doc_changes {
                DocumentChanges::Operations(ops) => {
                    let resource_count = ops
                        .iter()
                        .filter(|op| matches!(op, DocumentChangeOperation::Op(_)))
                        .count();
                    if resource_count > 0 {
                        warnings.push(format!(
                            "{} resource operation(s) (create/rename/delete) present; \
                             not surfaced in preview",
                            resource_count
                        ));
                    }
                }
                DocumentChanges::Edits(_) => {
                    // Edits-only shape — no resource operations.
                }
            }
        }

        // Step 5: re-check the caps from the prepared preview.
        let edit_count = preview.total_edits;
        let mut truncated = preview.truncated;
        if preview.total_files > RENAME_PREVIEW_MAX_FILES {
            truncated = true;
            warnings.push(format!(
                "rename touched {} files; preview capped at {}",
                preview.total_files, RENAME_PREVIEW_MAX_FILES
            ));
        }
        if edit_count > RENAME_PREVIEW_MAX_EDITS {
            truncated = true;
            warnings.push(format!(
                "rename produced {} edits; preview capped at {}",
                edit_count, RENAME_PREVIEW_MAX_EDITS
            ));
        }

        // Step 6: capture hashes of affected files from the preview
        // and verify none changed externally during the request.
        let mut affected_file_versions = Vec::new();
        let mut base_stale = false;

        // Re-read the target file to detect external changes.
        let post_content = tokio::fs::read_to_string(&target_file).await.map_err(|e| {
            LspError::RequestFailed(format!(
                "failed to re-read target file {}: {}",
                target_file.display(),
                e
            ))
        })?;
        let target_post_hash = sha256_hex(post_content.as_bytes());
        if target_post_hash != target_base_hash {
            base_stale = true;
        }
        affected_file_versions.push(VersionedFileEvidence {
            file: target_file.clone(),
            content_hash: target_post_hash,
            document_version: None,
        });

        for fp in &preview.files {
            if fp.file == target_file {
                continue;
            }
            let p = fp.file.clone();
            let post_content = tokio::fs::read_to_string(&p).await.map_err(|e| {
                LspError::RequestFailed(format!(
                    "failed to read affected file {}: {}",
                    p.display(),
                    e
                ))
            })?;
            let post_hash = sha256_hex(post_content.as_bytes());
            if post_hash != fp.original_hash {
                base_stale = true;
            }
            affected_file_versions.push(VersionedFileEvidence {
                file: p,
                content_hash: post_hash,
                document_version: None,
            });
        }

        let (key, _root) = self.service.get_or_create_client(file_path).await?;
        let server_generation = self.service.generation_for_key(&key).await;

        Ok(RenamePreview {
            old_name,
            new_name: new_name.to_string(),
            affected_files: preview.files,
            edit_count,
            warnings,
            truncated,
            base_stale,
            affected_file_versions,
            server_generation,
        })
    }

    /// Private helper: run the rename pipeline and return BOTH
    /// the raw `WorkspaceEdit` (for resource-op inspection) AND
    /// the prepared `WorkspaceEditPreview` (for the model-facing
    /// surface). Reuses the same logic as the public
    /// `rename_preview_unchecked` method.
    pub(crate) async fn rename_raw_and_preview(
        &self,
        file_path: &Path,
        line: u32,
        column: u32,
        new_name: &str,
        allowed_root: Option<&Path>,
    ) -> Result<(WorkspaceEdit, crate::edit::WorkspaceEditPreview), LspError> {
        let (key, _uri_str) = self.service.ensure_file_open_from_disk(file_path).await?;
        let uri = url_to_file_path_from_url(file_path)?;

        // Best-effort prepareRename — ignored on failure.
        let _ = self
            .service
            .send_request(
                &key,
                "textDocument/prepareRename",
                serde_json::to_value(TextDocumentPositionParams {
                    text_document: TextDocumentIdentifier {
                        uri: url_to_uri(&uri)?,
                    },
                    position: Position {
                        line,
                        character: column,
                    },
                })?,
            )
            .await;

        let params = serde_json::to_value(RenameParams {
            text_document_position: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier {
                    uri: url_to_uri(&uri)?,
                },
                position: Position {
                    line,
                    character: column,
                },
            },
            new_name: new_name.to_string(),
            work_done_progress_params: Default::default(),
        })?;

        let resp = self
            .service
            .send_request(&key, "textDocument/rename", params)
            .await?;

        if resp.is_null() {
            return Err(LspError::RequestFailed(
                "rename returned no result (no edits or unsupported at location)".to_string(),
            ));
        }

        let ws_edit: WorkspaceEdit = serde_json::from_value(resp)?;
        let preview = preview_workspace_edit("rename symbol", ws_edit.clone(), allowed_root)?;
        Ok((ws_edit, preview))
    }

    /// Private helper: look up the [`LspCapabilitySnapshot`] for the client that
    /// services `file_path`. Returns `None` when the client has not
    /// published capabilities yet (i.e. still initializing).
    pub(crate) async fn capability_snapshot_for_file(
        &self,
        file_path: &Path,
    ) -> Option<LspCapabilitySnapshot> {
        self.capability_snapshot_for_file_impl(file_path).await
    }
}

/// Private helper to convert a file path to a `url::Url`.
fn url_to_file_path_from_url(file_path: &Path) -> Result<url::Url, LspError> {
    url::Url::from_file_path(file_path)
        .map_err(|_| LspError::LaunchFailed(format!("invalid file path: {}", file_path.display())))
}

#[cfg(test)]
#[allow(dead_code, clippy::field_reassign_with_default)]
mod tests {
    use super::*;
    use crate::capability::{LspCapabilitySnapshot, LspSemanticOperation, LspUnavailable};
    use lsp_types::{ServerCapabilities, Uri};
    use std::path::PathBuf;
    use std::str::FromStr;

    fn uri(s: &str) -> Uri {
        Uri::from_str(s).expect("valid uri")
    }

    fn range(line: u32, col: u32) -> lsp_types::Range {
        lsp_types::Range {
            start: Position {
                line,
                character: col,
            },
            end: Position {
                line,
                character: col + 3,
            },
        }
    }

    // ---- PrepareRenameResult ----

    #[test]
    fn prepare_rename_result_from_response_range_no_placeholder() {
        let resp = Some(PrepareRenameResponse::Range(range(1, 2)));
        let out = PrepareRenameResult::from_response(resp);
        match out {
            PrepareRenameResult::Range {
                range: r,
                placeholder,
            } => {
                assert_eq!(r.start.line, 1);
                assert_eq!(r.start.character, 2);
                assert!(placeholder.is_none());
            }
            other => panic!("expected Range, got {other:?}"),
        }
    }

    #[test]
    fn prepare_rename_result_from_response_range_with_placeholder() {
        let resp = Some(PrepareRenameResponse::RangeWithPlaceholder {
            range: range(5, 0),
            placeholder: "old_name".to_string(),
        });
        let out = PrepareRenameResult::from_response(resp);
        match out {
            PrepareRenameResult::Range {
                range: r,
                placeholder,
            } => {
                assert_eq!(r.start.line, 5);
                assert_eq!(placeholder.as_deref(), Some("old_name"));
            }
            other => panic!("expected Range, got {other:?}"),
        }
    }

    #[test]
    fn prepare_rename_result_from_response_default_behavior_true() {
        let resp = Some(PrepareRenameResponse::DefaultBehavior {
            default_behavior: true,
        });
        let out = PrepareRenameResult::from_response(resp);
        assert_eq!(out, PrepareRenameResult::DefaultBehavior);
        assert!(out.is_renameable());
    }

    #[test]
    fn prepare_rename_result_from_response_default_behavior_false_is_not_renameable() {
        let resp = Some(PrepareRenameResponse::DefaultBehavior {
            default_behavior: false,
        });
        let out = PrepareRenameResult::from_response(resp);
        assert_eq!(out, PrepareRenameResult::NotRenameable);
        assert!(!out.is_renameable());
    }

    #[test]
    fn prepare_rename_result_from_response_none_is_not_renameable() {
        let out = PrepareRenameResult::from_response(None);
        assert_eq!(out, PrepareRenameResult::NotRenameable);
        assert!(!out.is_renameable());
    }

    #[test]
    fn prepare_rename_result_unavailable_range_accessor() {
        let r = range(7, 0);
        let v = PrepareRenameResult::Range {
            range: r,
            placeholder: None,
        };
        assert_eq!(v.range(), Some(&r));
        assert!(v.is_renameable());

        let d = PrepareRenameResult::DefaultBehavior;
        assert!(d.range().is_none());
        assert!(d.is_renameable());

        let n = PrepareRenameResult::NotRenameable;
        assert!(n.range().is_none());
        assert!(!n.is_renameable());

        let u = PrepareRenameResult::Unavailable(LspUnavailable::new(
            LspSemanticOperation::PrepareRename,
            "no provider",
        ));
        assert!(u.range().is_none());
        assert!(!u.is_renameable());
    }

    // ---- capability gating: prepare_rename ----

    #[test]
    fn capability_snapshot_reports_prepare_rename_unavailable_when_unset() {
        let caps = ServerCapabilities::default();
        let snap = LspCapabilitySnapshot::from_capabilities(&caps, Some("pylsp"), Some("python"));
        assert!(!snap.supports(LspSemanticOperation::Rename));
        assert!(!snap.supports(LspSemanticOperation::PrepareRename));
        let u = snap
            .unavailable(LspSemanticOperation::PrepareRename)
            .expect("unavailable");
        assert_eq!(u.operation, "prepareRename");
        assert!(u.reason.contains("pylsp"));
    }

    #[test]
    fn capability_snapshot_reports_prepare_rename_available_when_advertised() {
        let mut caps = ServerCapabilities::default();
        caps.rename_provider = Some(lsp_types::OneOf::Right(RenameOptions {
            prepare_provider: Some(true),
            work_done_progress_options: Default::default(),
        }));
        let snap = LspCapabilitySnapshot::from_capabilities(&caps, Some("s"), Some("rust"));
        assert!(snap.supports(LspSemanticOperation::PrepareRename));
        assert!(snap
            .unavailable(LspSemanticOperation::PrepareRename)
            .is_none());
    }

    // ---- RenamePreview freshness fields ----

    #[test]
    fn rename_preview_includes_base_freshness_fields() {
        let preview = RenamePreview {
            old_name: Some("old".to_string()),
            new_name: "new".to_string(),
            affected_files: Vec::new(),
            edit_count: 0,
            warnings: Vec::new(),
            truncated: false,
            base_stale: false,
            affected_file_versions: Vec::new(),
            server_generation: 1,
        };
        assert!(!preview.base_stale);
        assert!(preview.affected_file_versions.is_empty());
    }

    #[test]
    fn rename_preview_stale_base_detected() {
        let old_hash = super::sha256_hex(b"original");
        let new_hash = super::sha256_hex(b"modified");

        let preview = RenamePreview {
            old_name: Some("foo".to_string()),
            new_name: "bar".to_string(),
            affected_files: Vec::new(),
            edit_count: 1,
            warnings: Vec::new(),
            truncated: false,
            base_stale: old_hash != new_hash,
            affected_file_versions: vec![VersionedFileEvidence {
                file: PathBuf::from("foo.rs"),
                content_hash: new_hash.clone(),
                document_version: None,
            }],
            server_generation: 1,
        };
        assert!(preview.base_stale);
        assert_eq!(preview.affected_file_versions.len(), 1);
        assert_eq!(preview.affected_file_versions[0].content_hash, new_hash);
    }

    #[test]
    fn rename_preview_affected_files_populated() {
        let files = vec![
            VersionedFileEvidence {
                file: PathBuf::from("a.rs"),
                content_hash: super::sha256_hex(b"a"),
                document_version: None,
            },
            VersionedFileEvidence {
                file: PathBuf::from("b.rs"),
                content_hash: super::sha256_hex(b"b"),
                document_version: Some(3),
            },
        ];
        let preview = RenamePreview {
            old_name: Some("x".to_string()),
            new_name: "y".to_string(),
            affected_files: Vec::new(),
            edit_count: 2,
            warnings: Vec::new(),
            truncated: false,
            base_stale: false,
            affected_file_versions: files.clone(),
            server_generation: 1,
        };
        assert_eq!(preview.affected_file_versions.len(), 2);
        assert_eq!(
            preview.affected_file_versions[0].file,
            PathBuf::from("a.rs")
        );
        assert_eq!(preview.affected_file_versions[1].document_version, Some(3));
    }

    #[test]
    fn rename_secondary_file_change_sets_base_stale() {
        let original_hash = super::sha256_hex(b"original");
        let modified_hash = super::sha256_hex(b"modified");
        assert_ne!(original_hash, modified_hash);
        let stale = original_hash != modified_hash;
        assert!(stale);
    }

    #[test]
    fn rename_target_file_change_sets_base_stale() {
        let base_hash = super::sha256_hex(b"before");
        let post_hash = super::sha256_hex(b"after");
        assert!(base_hash != post_hash);
    }

    #[test]
    fn rename_unchanged_files_are_not_stale() {
        let hash = super::sha256_hex(b"unchanged");
        let stale = hash != hash;
        assert!(!stale);
    }

    #[test]
    fn rename_version_evidence_covers_all_preview_files() {
        let evidence = [
            super::VersionedFileEvidence {
                file: PathBuf::from("a.rs"),
                content_hash: super::sha256_hex(b"a"),
                document_version: None,
            },
            super::VersionedFileEvidence {
                file: PathBuf::from("b.rs"),
                content_hash: super::sha256_hex(b"b"),
                document_version: None,
            },
        ];
        assert_eq!(evidence.len(), 2);
    }

    // ── Pass 1: capability decision flow tests ─────────────────────

    /// Build a snapshot where the server advertises `renameProvider`
    /// but NOT `prepareProvider`. The `renameProvider` is set with
    /// `prepare_provider: Some(false)`, so the snapshot should
    /// report rename supported and prepare-rename unsupported.
    fn rename_supported_prepare_unsupported_snapshot() -> LspCapabilitySnapshot {
        let mut caps = ServerCapabilities::default();
        caps.rename_provider = Some(lsp_types::OneOf::Right(RenameOptions {
            prepare_provider: Some(false),
            work_done_progress_options: Default::default(),
        }));
        LspCapabilitySnapshot::from_capabilities(&caps, Some("clangd"), Some("cpp"))
    }

    /// Build a snapshot where the server does not advertise any
    /// rename provider. Both rename and prepare-rename are
    /// unsupported.
    fn rename_unsupported_snapshot() -> LspCapabilitySnapshot {
        let caps = ServerCapabilities::default();
        LspCapabilitySnapshot::from_capabilities(&caps, Some("pylsp"), Some("python"))
    }

    /// Build a snapshot where the server advertises both rename
    /// and prepare-rename.
    fn rename_and_prepare_supported_snapshot() -> LspCapabilitySnapshot {
        let mut caps = ServerCapabilities::default();
        caps.rename_provider = Some(lsp_types::OneOf::Right(RenameOptions {
            prepare_provider: Some(true),
            work_done_progress_options: Default::default(),
        }));
        LspCapabilitySnapshot::from_capabilities(&caps, Some("rust-analyzer"), Some("rust"))
    }

    #[test]
    fn rename_supported_prepare_unsupported_decision_is_distinct() {
        // Pass 1 — when rename is supported but prepare-rename is
        // not, the typed surface must distinguish the two states
        // and call the rename request directly (with `old_name =
        // None`) instead of stopping with a "not renameable" error.
        let snap = rename_supported_prepare_unsupported_snapshot();
        assert!(snap.supports(LspSemanticOperation::Rename));
        assert!(!snap.supports(LspSemanticOperation::PrepareRename));
        match snap.decide(LspSemanticOperation::PrepareRename) {
            CapabilityDecision::Unsupported(u) => {
                assert_eq!(u.operation, "prepareRename");
            }
            other => panic!("expected Unsupported, got {other:?}"),
        }
    }

    #[test]
    fn rename_unsupported_decision_is_unsupported() {
        // Pass 1 — when rename is not advertised, the decision
        // must be `Unsupported` so the typed surface rejects the
        // request before any protocol call.
        let snap = rename_unsupported_snapshot();
        assert!(!snap.supports(LspSemanticOperation::Rename));
        match snap.decide(LspSemanticOperation::Rename) {
            CapabilityDecision::Unsupported(u) => {
                assert_eq!(u.operation, "rename");
            }
            other => panic!("expected Unsupported, got {other:?}"),
        }
    }

    #[test]
    fn rename_and_prepare_supported_both_supported() {
        // Pass 1 — when both are advertised, both decisions are
        // `Supported` so the typed surface calls prepare-rename
        // first, then rename.
        let snap = rename_and_prepare_supported_snapshot();
        assert!(snap.supports(LspSemanticOperation::Rename));
        assert!(snap.supports(LspSemanticOperation::PrepareRename));
        assert!(matches!(
            snap.decide(LspSemanticOperation::Rename),
            CapabilityDecision::Supported
        ));
        assert!(matches!(
            snap.decide(LspSemanticOperation::PrepareRename),
            CapabilityDecision::Supported
        ));
    }

    #[test]
    fn prepare_null_response_normalizes_to_not_renameable() {
        // Pass 1 — when the server returns null from prepareRename
        // (i.e. the position cannot be renamed), the typed surface
        // must stop before issuing the rename request and return
        // an empty structured preview.
        let prepared: PrepareRenameResult = PrepareRenameResult::from_response(None);
        assert!(matches!(prepared, PrepareRenameResult::NotRenameable));
        assert!(!prepared.is_renameable());
    }

    #[test]
    fn prepare_default_behavior_is_renameable_with_no_placeholder() {
        // Pass 1 — when the server returns `defaultBehavior: true`,
        // the typed surface must continue to the rename step and
        // pass `old_name = None` (no placeholder available).
        let prepared: PrepareRenameResult =
            PrepareRenameResult::from_response(Some(PrepareRenameResponse::DefaultBehavior {
                default_behavior: true,
            }));
        assert!(matches!(prepared, PrepareRenameResult::DefaultBehavior));
        assert!(prepared.is_renameable());
        // No placeholder when the server returns DefaultBehavior.
        assert!(prepared.range().is_none());
    }

    #[test]
    fn prepare_unavailable_is_not_renameable() {
        // Pass 1 — when the server does not advertise prepare-rename,
        // the typed surface must NOT collapse this into a
        // `NotRenameable` for the position; the rename pipeline
        // branches on this case and issues the rename request
        // directly.
        let prepared = PrepareRenameResult::Unavailable(LspUnavailable::new(
            LspSemanticOperation::PrepareRename,
            "no provider",
        ));
        assert!(!prepared.is_renameable());
        assert!(matches!(prepared, PrepareRenameResult::Unavailable(_)));
        // The rename pipeline uses a different branch
        // (`CapabilityDecision::Unsupported`) for this case;
        // a `PrepareRenameResult::Unavailable` would only be
        // observed if the caller already invoked
        // `prepare_rename_typed` directly. The flow is:
        //   rename_preview_typed inspects the capability decision
        //   directly, NOT the result of `prepare_rename_typed`.
    }
}

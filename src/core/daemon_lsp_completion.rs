//! M003: the native LSP **completion read** surface (ADR-0021, Option B).
//!
//! One bounded, inert, warm-server-only completion read. This module owns the
//! completion request the way `daemon_lsp.rs` owns the M006-B reads: same
//! warm-only shape, same daemon-resolved root, same containment primitive,
//! same `via_session + file.read` gate (the descriptor lives in
//! `codegg-core`, the check in the daemon preamble — neither is repeated
//! here).
//!
//! Three properties are load-bearing:
//!
//! - **Warm-only.** The handler resolves the client with
//!   `find_existing_client_for_root_hint` and queries through
//!   `completion_for_warm_client`, which takes an already-warm key. No call
//!   path reaches `get_or_create_client`: a cold read returns `NotReady` and
//!   cannot spawn a language server.
//! - **Revision-bound.** The caller's revision is a precondition against the
//!   daemon-current revision (`Stale` on mismatch), and the daemon-current
//!   revision is checked against the last revision delivered to the language
//!   server (`Unsynced` on mismatch). An unsynchronized draft never yields an
//!   applicable candidate.
//! - **Inert.** Candidates carry display strings plus at most one validated
//!   single-document plain-text edit. Snippets, additional edits, commands,
//!   out-of-snapshot ranges and oversized edits stay visible with a reason
//!   and can never be applied from this surface. Nothing here mutates a
//!   document, a disk file, or a server.
//!
//! Denial is not a status: precondition failures are errors, `NotReady`
//! means "no warm server", and a warm-server query failure is `ServerFailed`.

use codegg_protocol::lsp::{
    LspCompletionApplicabilityDto, LspCompletionCandidateDto, LspCompletionEditDto,
    LspCompletionRequestDto, LspCompletionResultDto, LspCompletionUnsupportedReason,
    LspReadInvalid, MAX_LSP_COMPLETION_DETAIL_CHARS, MAX_LSP_COMPLETION_LABEL_CHARS,
    MAX_LSP_COMPLETION_RESULT_BYTES,
};
use egglsp::operations::{ClassifiedCompletionItem, CompletionSupport, CompletionUnsupportedKind};

use crate::core::daemon::CoreDaemon;
use crate::core::daemon_lsp::LspReadDenial;
use crate::protocol::core::CoreResponse;

/// Outcome of the revision/sync comparison, without any I/O.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SyncVerdict {
    /// Caller, daemon and language server agree: a query may proceed.
    ///
    /// The handler's fallthrough case rather than a named arm, so unit tests
    /// are its only constructor; the allow keeps that honest instead of
    /// hiding it.
    #[allow(dead_code)]
    Synced,
    /// The caller's revision is not the daemon's current revision.
    Stale,
    /// The daemon's current revision never reached the language server.
    Unsynced,
}

/// Pure revision/sync comparison for one completion read.
///
/// `request_revision` is the caller's view of the daemon revision,
/// `daemon_revision` is current, and `lsp_synced_revision` is the revision
/// last delivered to the server, if any.
pub(crate) fn completion_sync_verdict(
    request_revision: u64,
    daemon_revision: u64,
    lsp_synced_revision: Option<u64>,
) -> SyncVerdict {
    if request_revision != daemon_revision {
        return SyncVerdict::Stale;
    }
    if lsp_synced_revision != Some(daemon_revision) {
        return SyncVerdict::Unsynced;
    }
    SyncVerdict::Synced
}

/// Truncate a display string to a character cap, plainly.
///
/// Mirrors the hover read's truncation: a plain take, no ellipsis markup, so
/// untrusted server text is never mistaken for protocol syntax.
fn truncate_display(text: String, max_chars: usize) -> String {
    if text.chars().count() > max_chars {
        text.chars().take(max_chars).collect()
    } else {
        text
    }
}

fn unsupported_reason(kind: CompletionUnsupportedKind) -> LspCompletionUnsupportedReason {
    match kind {
        CompletionUnsupportedKind::Snippet => LspCompletionUnsupportedReason::Snippet,
        CompletionUnsupportedKind::AdditionalTextEdits => {
            LspCompletionUnsupportedReason::AdditionalEdits
        }
        CompletionUnsupportedKind::Command => LspCompletionUnsupportedReason::Command,
        CompletionUnsupportedKind::OutOfSnapshotRange => LspCompletionUnsupportedReason::OutOfRange,
        CompletionUnsupportedKind::InsertAndReplace => {
            LspCompletionUnsupportedReason::InsertAndReplace
        }
    }
}

/// Map one classified item to its DTO, enforcing the protocol's display caps
/// and the replacement-text byte bound.
///
/// Display strings truncate; an oversized edit becomes `EditTooLarge` rather
/// than being cut, because cutting an edit changes what would be inserted.
pub(crate) fn candidate_to_dto(item: ClassifiedCompletionItem) -> LspCompletionCandidateDto {
    let applicability = match item.support {
        CompletionSupport::Supported(edit) => {
            if edit.new_text.len() > codegg_protocol::lsp::MAX_LSP_COMPLETION_EDIT_BYTES {
                LspCompletionApplicabilityDto::NonApplicable {
                    reason: LspCompletionUnsupportedReason::EditTooLarge,
                }
            } else {
                LspCompletionApplicabilityDto::Applicable {
                    edit: LspCompletionEditDto {
                        start_line: edit.start_line,
                        start_character: edit.start_character,
                        end_line: edit.end_line,
                        end_character: edit.end_character,
                        new_text: edit.new_text,
                    },
                }
            }
        }
        CompletionSupport::Unsupported(kind) => LspCompletionApplicabilityDto::NonApplicable {
            reason: unsupported_reason(kind),
        },
    };
    LspCompletionCandidateDto {
        label: truncate_display(item.label, MAX_LSP_COMPLETION_LABEL_CHARS),
        detail: item
            .detail
            .map(|detail| truncate_display(detail, MAX_LSP_COMPLETION_DETAIL_CHARS)),
        kind: item.kind,
        filter_text: item
            .filter_text
            .map(|text| truncate_display(text, MAX_LSP_COMPLETION_LABEL_CHARS)),
        sort_text: item
            .sort_text
            .map(|text| truncate_display(text, MAX_LSP_COMPLETION_LABEL_CHARS)),
        applicability,
    }
}

/// Estimated wire weight of one candidate DTO, for the result byte budget.
fn candidate_bytes(candidate: &LspCompletionCandidateDto) -> usize {
    let edit_bytes = match &candidate.applicability {
        LspCompletionApplicabilityDto::Applicable { edit } => edit.new_text.len() + 16,
        LspCompletionApplicabilityDto::NonApplicable { .. } => 4,
    };
    candidate.label.len()
        + candidate.detail.as_deref().map_or(0, str::len)
        + candidate.kind.as_deref().map_or(0, str::len)
        + candidate.filter_text.as_deref().map_or(0, str::len)
        + candidate.sort_text.as_deref().map_or(0, str::len)
        + edit_bytes
}

/// Build the bounded candidate list from classified items.
///
/// Takes at most `max_candidates` in server order, then stops at the result
/// byte budget. Returns the list and whether anything was left out, so the
/// caller can set `truncated` as a measured fact.
pub(crate) fn cap_completion_candidates(
    classified: Vec<ClassifiedCompletionItem>,
    max_candidates: usize,
) -> (Vec<LspCompletionCandidateDto>, bool) {
    let mut candidates = Vec::new();
    let mut bytes = 0usize;
    let mut truncated = false;
    for item in classified.into_iter().take(max_candidates) {
        let dto = candidate_to_dto(item);
        bytes += candidate_bytes(&dto);
        if bytes > MAX_LSP_COMPLETION_RESULT_BYTES {
            truncated = true;
            break;
        }
        candidates.push(dto);
    }
    (candidates, truncated)
}

impl CoreDaemon {
    /// Answer one bounded inert LSP completion read.
    ///
    /// Order matters and is not incidental: validate the shape, resolve the
    /// daemon-owned root, resolve the target, bind the attachment to the
    /// session's workspace, check locator coherence, check the revision
    /// precondition, and only then ask whether a server is warm. Nothing
    /// reaches a language server until all of those have passed, and nothing
    /// here starts one.
    pub(crate) async fn lsp_completion_get(
        &self,
        request: LspCompletionRequestDto,
    ) -> Result<LspCompletionResultDto, LspReadDenial> {
        request.validate().map_err(|invalid| match invalid {
            LspReadInvalid::InvalidAttachment => LspReadDenial::AttachmentNotFound,
            LspReadInvalid::InvalidPath => LspReadDenial::InvalidPath,
            _ => LspReadDenial::IncoherentRequest,
        })?;
        let root = self.lsp_workspace_root(&request.session_id).await?;
        let target = Self::resolve_target(&root, &request.path)?;
        let binding = self
            .documents
            .completion_binding(&request.document_id)
            .await
            .ok_or(LspReadDenial::AttachmentNotFound)?;
        // The attachment must live under the session's workspace. Compare
        // canonical roots, not strings: two spellings of one directory must
        // not read as two authorities.
        let attachment_workspace =
            codegg_core::workspace::WorkspaceId::parse(&binding.workspace_id)
                .map_err(|_| LspReadDenial::AttachmentNotFound)?;
        let attachment_root = self
            .workspaces
            .resolve(&attachment_workspace)
            .await
            .ok_or(LspReadDenial::AttachmentNotFound)?;
        if attachment_root.canonical_root != root {
            return Err(LspReadDenial::AttachmentNotFound);
        }
        // The path and attachment locators must agree: disagreeing locators
        // are a client bug, rejected before any server contact.
        if binding.relative_path != request.path {
            return Err(LspReadDenial::IncoherentRequest);
        }
        let document_id = request.document_id.clone();
        // One matrix for both revision verdicts, evaluated in precedence
        // order: the `Stale` arm fires here on pure caller state, without any
        // server contact, while the `Unsynced` arm below only means something
        // against a warm server.
        let verdict = completion_sync_verdict(
            request.revision,
            binding.revision,
            binding.lsp_synced_revision,
        );
        if verdict == SyncVerdict::Stale {
            return Ok(LspCompletionResultDto::stale(
                document_id,
                binding.revision,
                binding.lsp_synced_revision,
            ));
        }
        let service = self.lsp_service()?;
        // Warm-only. This is the whole of ADR-0021 §2: with no client running
        // for this root the answer is `NotReady` — never a create, never an
        // empty success that would read as "no suggestions". A lookup miss is
        // a first-class outcome, not a denial: cold is not forbidden. Cold
        // outranks `Unsynced`: a missing server means "not ready", full stop.
        let (key, _) = match service
            .find_existing_client_for_root_hint(Some(&root), None)
            .await
        {
            Ok(found) => found,
            Err(_) => return Ok(LspCompletionResultDto::not_ready(document_id)),
        };
        // The sync witness only means something against a warm server: the
        // server must have seen exactly this revision.
        if verdict == SyncVerdict::Unsynced {
            return Ok(LspCompletionResultDto::unsynced(
                document_id,
                binding.revision,
                binding.lsp_synced_revision,
            ));
        }
        // Capability: a warm server without a completion provider is
        // `Unsupported`, not `NotReady`; a still-warming one is `NotReady`.
        match service
            .capability_decision(&key, egglsp::capability::LspSemanticOperation::Completion)
            .await
        {
            egglsp::capability::CapabilityDecision::Supported => {}
            egglsp::capability::CapabilityDecision::Unsupported(_) => {
                return Ok(LspCompletionResultDto::unsupported(
                    document_id,
                    binding.revision,
                    binding.lsp_synced_revision,
                ));
            }
            egglsp::capability::CapabilityDecision::Unknown { .. } => {
                return Ok(LspCompletionResultDto::not_ready(document_id));
            }
        }
        let operations = crate::lsp::operations::LspOperations::new(service.clone());
        let (items, is_incomplete) = operations
            .completion_for_warm_client(
                &key,
                &target,
                request.line,
                request.character,
                request.trigger_character.clone(),
            )
            .await
            .map_err(|_| LspReadDenial::ServerFailed)?;
        // The request was validated, so `capped_max_candidates` cannot exceed
        // the protocol bound; the clamp below is defense in depth.
        let cap = request.capped_max_candidates();
        let item_over_cap = items.len() > cap;
        let classified = egglsp::operations::classify_completion_items(
            &items[..items.len().min(cap)],
            &binding.text,
            request.line,
            request.character,
        );
        let (candidates, bytes_truncated) = cap_completion_candidates(classified, cap);
        Ok(LspCompletionResultDto::ready(
            document_id,
            binding.revision,
            Some(binding.revision),
            candidates,
            item_over_cap || bytes_truncated,
            is_incomplete,
        ))
    }
}

/// Route one completion request the way the read surface routes reads:
/// denials become errors, never `NotReady`.
pub(crate) async fn completion_response(
    daemon: &CoreDaemon,
    request: LspCompletionRequestDto,
) -> CoreResponse {
    match daemon.lsp_completion_get(request).await {
        Ok(result) => CoreResponse::LspCompletionResult { result },
        Err(denial) => CoreResponse::Error {
            code: denial.code().into(),
            message: denial.message().into(),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use egglsp::operations::InertCompletionEdit;

    #[test]
    fn sync_verdict_prefers_stale_over_unsynced() {
        // A caller behind the daemon is stale even when the daemon is also
        // behind the server: rebase first, then wait for sync.
        assert_eq!(completion_sync_verdict(7, 9, Some(4)), SyncVerdict::Stale);
        assert_eq!(completion_sync_verdict(7, 9, Some(9)), SyncVerdict::Stale);
    }

    #[test]
    fn sync_verdict_is_unsynced_until_the_server_saw_this_revision() {
        assert_eq!(
            completion_sync_verdict(9, 9, Some(8)),
            SyncVerdict::Unsynced
        );
        assert_eq!(completion_sync_verdict(9, 9, None), SyncVerdict::Unsynced);
    }

    #[test]
    fn sync_verdict_is_synced_only_on_exact_agreement() {
        assert_eq!(completion_sync_verdict(9, 9, Some(9)), SyncVerdict::Synced);
    }

    fn supported(label: &str, new_text: &str) -> ClassifiedCompletionItem {
        ClassifiedCompletionItem {
            label: label.to_string(),
            detail: None,
            kind: Some("function".to_string()),
            sort_text: None,
            filter_text: None,
            support: CompletionSupport::Supported(InertCompletionEdit {
                start_line: 0,
                start_character: 0,
                end_line: 0,
                end_character: 0,
                new_text: new_text.to_string(),
            }),
        }
    }

    fn unsupported_snippet(label: &str) -> ClassifiedCompletionItem {
        ClassifiedCompletionItem {
            label: label.to_string(),
            detail: None,
            kind: None,
            sort_text: None,
            filter_text: None,
            support: CompletionSupport::Unsupported(CompletionUnsupportedKind::Snippet),
        }
    }

    #[test]
    fn an_oversized_edit_is_non_applicable_never_truncated() {
        let big = "x".repeat(codegg_protocol::lsp::MAX_LSP_COMPLETION_EDIT_BYTES + 1);
        let dto = candidate_to_dto(supported("big", &big));
        assert_eq!(
            dto.applicability,
            LspCompletionApplicabilityDto::NonApplicable {
                reason: LspCompletionUnsupportedReason::EditTooLarge,
            }
        );
        assert_eq!(dto.label, "big");
    }

    #[test]
    fn display_strings_truncate_but_the_edit_survives() {
        let long_label = "l".repeat(MAX_LSP_COMPLETION_LABEL_CHARS + 50);
        let dto = candidate_to_dto(supported(&long_label, "short"));
        assert_eq!(dto.label.chars().count(), MAX_LSP_COMPLETION_LABEL_CHARS);
        match dto.applicability {
            LspCompletionApplicabilityDto::Applicable { edit } => {
                assert_eq!(edit.new_text, "short");
            }
            LspCompletionApplicabilityDto::NonApplicable { reason } => {
                panic!("edit must survive display truncation: {reason:?}")
            }
        }
    }

    #[test]
    fn unsupported_reasons_survive_the_dto_mapping() {
        let dto = candidate_to_dto(unsupported_snippet("snip"));
        assert_eq!(
            dto.applicability,
            LspCompletionApplicabilityDto::NonApplicable {
                reason: LspCompletionUnsupportedReason::Snippet,
            }
        );
    }

    #[test]
    fn capping_takes_the_first_n_in_server_order() {
        // The count truncation itself is measured by the caller (`items.len()
        // > cap`); this function only enforces the take and the byte budget.
        let items = vec![
            supported("a", "a"),
            supported("b", "b"),
            supported("c", "c"),
        ];
        let (candidates, truncated) = cap_completion_candidates(items, 2);
        assert_eq!(candidates.len(), 2);
        assert_eq!(candidates[0].label, "a");
        assert_eq!(candidates[1].label, "b");
        assert!(!truncated);
    }

    #[test]
    fn the_byte_budget_truncates_as_a_measured_fact() {
        let big = "y".repeat(8192);
        let items: Vec<_> = (0..40).map(|_| supported("big", &big)).collect();
        let (candidates, truncated) = cap_completion_candidates(items, 100);
        assert!(truncated, "320 KiB of edits must trip the 256 KiB budget");
        assert!(candidates.len() < 40);
        assert!(!candidates.is_empty());
    }

    #[test]
    fn server_order_survives_capping() {
        let items = vec![unsupported_snippet("z"), supported("a", "a")];
        let (candidates, _) = cap_completion_candidates(items, 10);
        assert_eq!(candidates[0].label, "z");
        assert_eq!(candidates[1].label, "a");
    }
}

//! M006-B: the native LSP **read** surface (ADR-0012).
//!
//! This module owns read authority only. The ADR-0008 write path,
//! `CoreRequest::LspPreviewApply`, stays in `daemon_goals.rs` and is not
//! reachable from here; splitting the two keeps the mutation boundary
//! independently readable rather than folded into a general "LSP" surface.
//!
//! Four properties are load-bearing, and each is structural rather than a
//! convention someone has to remember:
//!
//! - **Warm-only, structurally.** Every read resolves one warm client key
//!   through [`resolve_warm_client`] and hands *that key* to the query, which
//!   reaches the server through an `*_for_warm_client` entry point. Neither
//!   step can reach `get_or_create_client`, so a cold read returns `NotReady`
//!   and cannot spawn a language server. M013 replaced a weaker version of
//!   this: the gate resolved a client and then the query discarded it and
//!   resolved another from the file's own project root and language — a path
//!   that *could* create. Warm-only has to be a property of the call graph,
//!   not of a check someone remembered to write first.
//! - **Ambiguity is not readiness.** Several warm servers and no warm server
//!   are different facts, so [`WarmClient::Ambiguous`] answers as
//!   [`LspReadStatus::Ambiguous`] rather than as `NotReady`.
//! - **Daemon-resolved root.** The workspace root comes from the daemon's
//!   session row, never from the request. The request carries a
//!   workspace-*relative* path only, so a client cannot name a root.
//! - **Same containment as the write path.** Targets resolve through
//!   `crate::tool::util::validate_target_path`, the identical primitive
//!   `src/lsp/mutation.rs` uses to apply a preview, so a file that cannot be
//!   written also cannot be read.
//! - **Denial is not `NotReady`.** A precondition failure is an error
//!   response; `NotReady` means "no warm server". Conflating them would tell
//!   a caller that a forbidden request was merely early.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use codegg_protocol::lsp::{
    LspCodeActionDto, LspCodeActionUnavailable, LspDiagnosticsGetRequestDto,
    LspDiagnosticsResultDto, LspFormatPreviewDto, LspHighlightDto, LspPreviewCandidateDto,
    LspPreviewPatchDto, LspRangeDto, LspReadOperation, LspReadPayloadDto, LspReadRequestDto,
    LspReadResultDto, LspRenameFileDto, LspRenamePreviewDto, LspSignatureDto,
    LspSignatureParameterDto, LspSymbolDto, MAX_LSP_CODE_ACTIONS, MAX_LSP_PREVIEW_CANDIDATE_BYTES,
    MAX_LSP_PREVIEW_CANDIDATE_PATCHES, MAX_LSP_PREVIEW_CANDIDATE_PATCH_BYTES,
    MAX_LSP_READ_CANDIDATE_BYTES,
};

use crate::core::daemon::CoreDaemon;
use crate::error::AppError;
use crate::lsp::service::LspService;
use crate::protocol::core::{CoreEvent, CoreRequest, CoreResponse};

/// Why an LSP read could not be served.
///
/// These are daemon preconditions, deliberately distinct from a language
/// server's own error and from `NotReady`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LspReadDenial {
    /// The operation's fields did not match its shape.
    IncoherentRequest,
    /// The relative path was empty or absolute.
    InvalidPath,
    /// The path did not resolve inside the session's workspace.
    OutsideWorkspace,
    /// No session row bound this locator to a workspace.
    SessionNotFound,
    /// The daemon has no `LspService` in this process.
    ServiceUnavailable,
    /// M003: the document attachment id is unknown, or the attachment does
    /// not live under the session's workspace.
    AttachmentNotFound,
    /// M003: the language server request failed after all preconditions
    /// passed. Distinct from `NotReady`: the server was warm, but the query
    /// itself errored.
    ServerFailed,
}

impl LspReadDenial {
    /// A denial message naming no project, principal, or secret.
    ///
    /// Non-enumerability is a hard contract: a denial must be
    /// indistinguishable from an absent resource, or it becomes an oracle for
    /// which sessions and workspaces exist. `OutsideWorkspace` and
    /// `SessionNotFound` therefore share one message *and* one code.
    pub fn message(self) -> &'static str {
        match self {
            Self::IncoherentRequest => "the read request is not valid for this operation",
            Self::InvalidPath => "the requested path is not a valid workspace-relative path",
            Self::OutsideWorkspace | Self::SessionNotFound | Self::AttachmentNotFound => {
                "the requested path is not available"
            }
            Self::ServiceUnavailable => "LSP is unavailable for this request",
            Self::ServerFailed => "the language server request failed",
        }
    }

    pub fn code(self) -> &'static str {
        match self {
            Self::IncoherentRequest | Self::InvalidPath => "invalid_lsp_read",
            Self::OutsideWorkspace | Self::SessionNotFound | Self::AttachmentNotFound => {
                "lsp_read_not_found"
            }
            Self::ServiceUnavailable => "lsp_unavailable",
            Self::ServerFailed => "lsp_server_failed",
        }
    }
}

/// Which warm language server answers one read.
///
/// The resolver in `egglsp` can only answer "exactly one" or `Err`, and the
/// daemon used to map every `Err` to `NotReady`. That made a polyglot
/// workspace with two healthy servers read exactly like a cold one: same
/// status, same absent payload, same "nothing to see yet". A client cannot
/// tell a user which of the two is wrong, and neither can it name the
/// server the user should pick.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum WarmClient {
    /// No client is running for this root. `NotReady`, and never a create
    /// (ADR-0012 §3).
    Cold,
    /// Exactly one client, or one selected by an explicit id or by an
    /// unambiguous extension match.
    Resolved { key: String },
    /// More than one warm client and nothing selected one of them.
    ///
    /// Reported rather than guessed. An LSP answer from the wrong server is
    /// worse than an honest "I cannot choose", because it is wrong in a way
    /// that looks right.
    Ambiguous { candidates: Vec<String> },
}

/// The `server_id` inside a `"{root}:{server_id}"` client key.
///
/// Stripped with the prefix the key was built with, so the server id is
/// recovered by construction rather than by splitting on the last colon —
/// which would be a guess about Windows drive letters.
fn server_id_from_key(key: &str, root: &Path) -> String {
    key.strip_prefix(&format!("{}:", root.display()))
        .unwrap_or(key)
        .to_string()
}

/// Resolve which warm client answers a read.
///
/// The order is deliberate, and every step can only narrow the choice:
///
/// 1. **Explicit `server_id`.** A caller that names a server gets it, or
///    nothing. A named-but-cold server is `Cold`, not `Ambiguous`: there is
///    genuinely no warm server for what was asked for, and "several" would be
///    false.
/// 2. **Extension match**, and only when exactly one warm client claims the
///    target's extension. Two claimants is a polyglot answer, not a pick —
///    [`egglsp::server::servers_for_extension`] returns every claimant rather
///    than the first, precisely so this step can see the second.
/// 3. **Exactly one warm client.** The common case.
/// 4. **Otherwise ambiguous**, naming the candidates.
///
/// Steps 2 and 3 are skipped entirely when an explicit id was given, so a
/// caller can always resolve an ambiguity the extension cannot.
pub(crate) async fn resolve_warm_client(
    service: &LspService,
    root: &Path,
    target: Option<&Path>,
    server_id: Option<&str>,
) -> WarmClient {
    let candidates = service.existing_client_keys_for_root(root).await;
    if candidates.is_empty() {
        return WarmClient::Cold;
    }

    let warm = |wanted: &str| {
        candidates
            .iter()
            .find(|key| key.as_str() == wanted)
            .cloned()
    };

    if let Some(sid) = server_id {
        let wanted = format!("{}:{}", root.display(), sid);
        return match warm(&wanted) {
            Some(key) => WarmClient::Resolved { key },
            None => WarmClient::Cold,
        };
    }

    if let Some(target) = target {
        if let Some(extension) = target.extension().and_then(|ext| ext.to_str()) {
            let claiming: Vec<String> = crate::lsp::server::servers_for_extension(extension)
                .into_iter()
                .filter_map(|server| warm(&format!("{}:{}", root.display(), server.id)))
                .collect();
            // Two warm servers both claiming this extension is a genuine
            // polyglot question, so it stays ambiguous rather than falling
            // back to "whichever key sorted first".
            if claiming.len() == 1 {
                return WarmClient::Resolved {
                    key: claiming.into_iter().next().unwrap(),
                };
            }
        }
    }

    match candidates.len() {
        1 => WarmClient::Resolved {
            key: candidates.into_iter().next().unwrap(),
        },
        _ => WarmClient::Ambiguous {
            candidates: candidates
                .into_iter()
                .map(|key| server_id_from_key(&key, root))
                .collect(),
        },
    }
}

impl CoreDaemon {
    /// Dispatch one M006-B LSP read request.
    pub(crate) async fn handle_lsp_request(
        &self,
        request: CoreRequest,
        trusted_client_id: &str,
    ) -> Result<CoreResponse, AppError> {
        match request {
            CoreRequest::LspReadGet { request } => match self.lsp_read_get(request).await {
                Ok(result) => Ok(CoreResponse::LspReadResult { result }),
                Err(denial) => Ok(CoreResponse::Error {
                    code: denial.code().into(),
                    message: denial.message().into(),
                }),
            },
            CoreRequest::LspDiagnosticsGet { request } => {
                let result = self.lsp_diagnostics_get(request).await;
                Ok(CoreResponse::LspDiagnosticsGetResult { result })
            }
            CoreRequest::LspDiagnosticsSubscribe { request } => {
                self.lsp_diagnostics_subscribe(request, trusted_client_id)
                    .await
            }
            CoreRequest::LspCompletionGet { request } => {
                Ok(super::daemon_lsp_completion::completion_response(self, request).await)
            }
            other => Ok(CoreResponse::Error {
                code: "unsupported_lsp_request".into(),
                message: format!("{other:?} is not an LSP read request"),
            }),
        }
    }

    /// Resolve the daemon-owned workspace root for a session locator.
    ///
    /// The locator is validated fail-closed before it reaches SQL: session ids
    /// are opaque and must never be usable as paths. The workspace comes from
    /// the session row, so a caller cannot name a root it does not own.
    pub(crate) async fn lsp_workspace_root(
        &self,
        session_id: &str,
    ) -> Result<PathBuf, LspReadDenial> {
        let Some(pool) = self.pool.clone() else {
            return Err(LspReadDenial::ServiceUnavailable);
        };
        if session_id.is_empty()
            || session_id.contains('/')
            || session_id.contains('\\')
            || codegg_core::context::SessionId::parse(session_id).is_err()
        {
            return Err(LspReadDenial::SessionNotFound);
        }
        let bound = sqlx::query_scalar::<_, Option<String>>(
            "SELECT workspace_id FROM session WHERE id = ?",
        )
        .bind(session_id)
        .fetch_optional(&pool)
        .await
        .map_err(|_| LspReadDenial::SessionNotFound)?;
        // A session may legitimately have no workspace bound (a standalone
        // local session). That is "not available", not a distinct error, so
        // it shares the non-enumerable denial with every other miss.
        let Some(workspace_id) = bound.flatten() else {
            return Err(LspReadDenial::SessionNotFound);
        };
        let workspace_id = codegg_core::workspace::WorkspaceId::new_unchecked(workspace_id);
        let Some(record) = self.workspaces.resolve(&workspace_id).await else {
            return Err(LspReadDenial::SessionNotFound);
        };
        Ok(record.canonical_root.clone())
    }

    /// The daemon's `LspService`, or a denial.
    pub(crate) fn lsp_service(&self) -> Result<Arc<LspService>, LspReadDenial> {
        self.deps
            .lsp_service
            .clone()
            .ok_or(LspReadDenial::ServiceUnavailable)
    }

    /// Resolve a workspace-relative path against the daemon-owned root using
    /// the same containment primitive as the write path.
    pub(crate) fn resolve_target(root: &Path, relative: &str) -> Result<PathBuf, LspReadDenial> {
        if relative.is_empty() || relative.starts_with('/') || relative.starts_with('\\') {
            return Err(LspReadDenial::InvalidPath);
        }
        crate::tool::util::validate_target_path(Path::new(relative), root)
            .map_err(|_| LspReadDenial::OutsideWorkspace)
    }

    /// Answer one bounded LSP read.
    ///
    /// Order matters and is not incidental: validate the request's shape,
    /// resolve the daemon-owned root, resolve the target, and only then ask
    /// whether a server is warm. Nothing reaches a language server until all
    /// four have passed.
    async fn lsp_read_get(
        &self,
        request: LspReadRequestDto,
    ) -> Result<LspReadResultDto, LspReadDenial> {
        request
            .validate()
            .map_err(|_| LspReadDenial::IncoherentRequest)?;
        let root = self.lsp_workspace_root(&request.session_id).await?;
        let service = self.lsp_service()?;

        let operation = request.operation;
        // An empty or whitespace-only `server_id` is treated as absent rather
        // than as a server literally named "". A caller that sent one by
        // accident then gets the ordinary resolution instead of a denial
        // about a server that cannot exist.
        let server_id = request
            .server_id
            .as_deref()
            .map(str::trim)
            .filter(|id| !id.is_empty());

        if operation == LspReadOperation::WorkspaceSymbols {
            // Project-wide, so there is no target to infer a language from.
            let query = request.query.as_deref().unwrap_or_default();
            let client = resolve_warm_client(service.as_ref(), &root, None, server_id).await;
            return Self::lsp_workspace_symbols(self, &service, &root, operation, client, query)
                .await;
        }

        let target = Self::resolve_target(&root, &request.path)?;

        // Warm-only, and this is the *same* resolution the query then uses.
        // That is the point: the gate used to ask the resolver one question
        // and the query answered a different one by re-resolving from the
        // file's own project root and language — a path that can reach a
        // create. See the module docs.
        let client =
            resolve_warm_client(service.as_ref(), &root, Some(target.as_path()), server_id).await;
        let key = match client {
            WarmClient::Cold => return Ok(LspReadResultDto::not_ready(operation)),
            WarmClient::Ambiguous { candidates } => {
                return Ok(LspReadResultDto::ambiguous(operation, candidates))
            }
            WarmClient::Resolved { key } => key,
        };

        self.lsp_anchored_read(&service, &root, operation, &key, &target, &request)
            .await
    }

    /// Turn a warm-client operation's error into the read family's outcome.
    ///
    /// A capability the server never advertised is an **answer**, not a
    /// failure: it becomes [`LspReadStatus::Unsupported`] naming the missing
    /// capability. Folding it into [`LspReadDenial::ServerFailed`] would tell
    /// the user their working language server broke, which is both false and
    /// unactionable — nothing they do will make a server advertise a provider
    /// it does not have.
    fn read_operation_error(
        operation: LspReadOperation,
        err: &crate::lsp::LspError,
    ) -> Result<LspReadResultDto, LspReadDenial> {
        match err {
            crate::lsp::LspError::Unavailable(_) => Ok(LspReadResultDto::unsupported(operation)),
            _ => Err(LspReadDenial::ServerFailed),
        }
    }

    /// Reads that name a target file, whether or not they take a position.
    ///
    /// Named `anchored` rather than `point` because half of these are
    /// file-scoped (document symbols, semantic tokens, format preview): a
    /// whole-document formatting request has no cursor to anchor to. The old
    /// name would have papered over exactly the distinction that made the
    /// original shape confusing.
    async fn lsp_anchored_read(
        &self,
        service: &Arc<LspService>,
        root: &Path,
        operation: LspReadOperation,
        key: &str,
        target: &Path,
        request: &LspReadRequestDto,
    ) -> Result<LspReadResultDto, LspReadDenial> {
        let operations = crate::lsp::operations::LspOperations::new(service.clone());
        let line = request.line.unwrap_or_default();
        let column = request.column.unwrap_or_default();

        // Every arm below goes through a `*_for_warm_client` entry point.
        // There is deliberately no file-path-addressed call left on this path:
        // `get_or_create_client` would let a read start a language server, and
        // would resolve a different root than the one the gate checked.
        let payload = match operation {
            LspReadOperation::Hover => {
                match operations
                    .hover_for_warm_client(key, target, line, column)
                    .await
                {
                    Ok(Some(text)) => {
                        let cap = codegg_protocol::lsp::MAX_LSP_HOVER_CHARS;
                        let truncated = text.chars().count() > cap;
                        LspReadPayloadDto::Hover {
                            text: text.chars().take(cap).collect(),
                            truncated,
                        }
                    }
                    // `None` from a *warm* server genuinely means "nothing at
                    // this position", which is a Ready empty result.
                    Ok(None) => LspReadPayloadDto::Hover {
                        text: String::new(),
                        truncated: false,
                    },
                    Err(err) => return Self::read_operation_error(operation, &err),
                }
            }
            // Declaration and implementation return the same `LocationLink`
            // shape as definition, so they share one conversion. They stay
            // separate operations because a server may offer any subset of the
            // three, and a client asking for one must not silently receive
            // another's answer.
            LspReadOperation::Declaration => {
                match operations
                    .declaration_for_warm_client(key, target, line, column)
                    .await
                {
                    Ok(links) => {
                        let (locations, truncated) = cap_locations(
                            links.iter().map(|link| location_link_to_dto(link, root)),
                        );
                        LspReadPayloadDto::Locations {
                            locations,
                            truncated,
                        }
                    }
                    Err(err) => return Self::read_operation_error(operation, &err),
                }
            }
            LspReadOperation::Implementation => {
                match operations
                    .implementation_for_warm_client(key, target, line, column)
                    .await
                {
                    Ok(links) => {
                        let (locations, truncated) = cap_locations(
                            links.iter().map(|link| location_link_to_dto(link, root)),
                        );
                        LspReadPayloadDto::Locations {
                            locations,
                            truncated,
                        }
                    }
                    Err(err) => return Self::read_operation_error(operation, &err),
                }
            }
            LspReadOperation::Definition => {
                match operations
                    .go_to_definition_for_warm_client(key, target, line, column)
                    .await
                {
                    Ok(links) => {
                        let (locations, truncated) = cap_locations(
                            links.iter().map(|link| location_link_to_dto(link, root)),
                        );
                        LspReadPayloadDto::Locations {
                            locations,
                            truncated,
                        }
                    }
                    Err(err) => return Self::read_operation_error(operation, &err),
                }
            }
            LspReadOperation::References => {
                match operations
                    .find_references_for_warm_client(key, target, line, column)
                    .await
                {
                    Ok(found) => {
                        let (locations, truncated) = cap_locations(
                            found.iter().map(|location| location_to_dto(location, root)),
                        );
                        LspReadPayloadDto::Locations {
                            locations,
                            truncated,
                        }
                    }
                    Err(err) => return Self::read_operation_error(operation, &err),
                }
            }
            LspReadOperation::DocumentHighlight => {
                match operations
                    .document_highlights_for_warm_client(key, target, line, column)
                    .await
                {
                    Ok(found) => {
                        // Document highlights are always inside the requested
                        // file, so the relative locator is the target's own —
                        // no URI is trusted for it.
                        let relative = workspace_relative_target(root, target);
                        let cap = codegg_protocol::lsp::MAX_LSP_HIGHLIGHTS;
                        let mut highlights: Vec<LspHighlightDto> = found
                            .into_iter()
                            .map(|highlight| {
                                let mut range = range_to_dto(highlight.range);
                                range.workspace_relative_path = relative.clone();
                                LspHighlightDto {
                                    range,
                                    kind: highlight.kind.map(highlight_kind_number),
                                }
                            })
                            .collect();
                        let truncated = highlights.len() > cap;
                        highlights.truncate(cap);
                        LspReadPayloadDto::Highlights {
                            highlights,
                            truncated,
                        }
                    }
                    Err(err) => return Self::read_operation_error(operation, &err),
                }
            }
            LspReadOperation::SignatureHelp => {
                match operations
                    .signature_help_typed_for_warm_client(key, target, line, column)
                    .await
                {
                    Ok(Some(summary)) => {
                        let cap = codegg_protocol::lsp::MAX_LSP_SIGNATURE_CHARS;
                        let bound = |text: &str| -> String {
                            let mut out: String = text.chars().take(cap).collect();
                            if text.chars().count() > cap {
                                out.push('…');
                            }
                            out
                        };
                        LspReadPayloadDto::SignatureHelp {
                            signatures: summary
                                .signatures
                                .into_iter()
                                .map(|signature| LspSignatureDto {
                                    label: bound(&signature.label),
                                    documentation: signature.documentation.as_deref().map(bound),
                                    parameters: signature
                                        .parameters
                                        .into_iter()
                                        .map(|parameter| LspSignatureParameterDto {
                                            label: bound(&parameter.label),
                                            documentation: parameter
                                                .documentation
                                                .as_deref()
                                                .map(bound),
                                        })
                                        .collect(),
                                })
                                .collect(),
                            active_signature: summary.active_signature,
                            active_parameter: summary.active_parameter,
                        }
                    }
                    // A warm server that has no signature at this position is
                    // a Ready empty answer, not a failure.
                    Ok(None) => LspReadPayloadDto::SignatureHelp {
                        signatures: Vec::new(),
                        active_signature: None,
                        active_parameter: None,
                    },
                    Err(err) => return Self::read_operation_error(operation, &err),
                }
            }
            LspReadOperation::DocumentSymbols => {
                match operations
                    .document_symbols_for_warm_client(key, target)
                    .await
                {
                    Ok(symbols) => {
                        let (symbols, truncated) = cap_symbols(symbols.into_iter().map(|symbol| {
                            let mut dto = document_symbol_to_dto(symbol);
                            dto.range.workspace_relative_path =
                                workspace_relative_target(root, target);
                            dto
                        }));
                        LspReadPayloadDto::Symbols { symbols, truncated }
                    }
                    Err(err) => return Self::read_operation_error(operation, &err),
                }
            }
            LspReadOperation::WorkspaceSymbols => {
                unreachable!("routed by the caller before reaching an anchored read")
            }
            LspReadOperation::SemanticTokens => {
                let cap = codegg_protocol::lsp::MAX_LSP_SEMANTIC_TOKENS;
                match operations
                    .semantic_tokens_for_warm_client(key, target, cap + 1)
                    .await
                {
                    Ok(tokens) => {
                        // Ask for one extra so `truncated` is a measured fact
                        // rather than a guess at the cap.
                        let truncated = tokens.len() > cap;
                        let tokens = tokens
                            .into_iter()
                            .take(cap)
                            .map(|token| codegg_protocol::lsp::LspSemanticTokenDto {
                                line: token.line,
                                character: token.start,
                                length: token.length,
                                token_type: token.token_type,
                                modifiers: token.modifiers,
                            })
                            .collect();
                        LspReadPayloadDto::SemanticTokens { tokens, truncated }
                    }
                    Err(err) => return Self::read_operation_error(operation, &err),
                }
            }
            // The two preview reads. They return what *would* change, and —
            // since M016 — the material the apply path needs when the preview
            // is complete enough to become a candidate. The write path itself
            // is ADR-0008's, in `daemon_goals.rs`; a preview never applies
            // anything.
            LspReadOperation::FormatPreview => {
                match operations
                    .format_preview_for_warm_client(key, target, Some(root))
                    .await
                {
                    Ok(preview) => {
                        let path = workspace_relative_target(root, target).unwrap_or_default();
                        // A truncated diff, an empty diff, or a file the
                        // daemon could not name relative to the workspace
                        // leaves the preview a preview.
                        let candidate =
                            if preview.truncated || preview.diff.is_empty() || path.is_empty() {
                                None
                            } else {
                                mint_preview_candidate(
                                    "formatting",
                                    &format!("Format {path}"),
                                    &format!("editor:format:{path}"),
                                    vec![(
                                        path.clone(),
                                        preview.diff.clone(),
                                        preview.before_hash.clone(),
                                    )],
                                )
                            };
                        LspReadPayloadDto::FormatPreview(LspFormatPreviewDto {
                            path,
                            edit_count: preview.edit_count,
                            before_hash: preview.before_hash,
                            after_hash: preview.after_hash,
                            diff: preview.diff,
                            truncated: preview.truncated,
                            base_stale: preview.base_stale,
                            candidate,
                        })
                    }
                    Err(err) => return Self::read_operation_error(operation, &err),
                }
            }
            LspReadOperation::RenamePreview => {
                // `validate` already proved `new_name` is present, non-empty
                // and within bound, so the default here is never what a
                // daemon-served request produces — it exists so a future
                // caller that skipped validation cannot send an empty name.
                let new_name = request.new_name.as_deref().unwrap_or_default();
                match operations
                    .rename_preview_for_warm_client(key, target, line, column, new_name, Some(root))
                    .await
                {
                    Ok(preview) => {
                        let cap = codegg_protocol::lsp::MAX_LSP_RENAME_FILES;
                        let mut truncated = preview.truncated;
                        let files: Vec<LspRenameFileDto> = preview
                            .affected_files
                            .iter()
                            // Containment is re-applied here rather than
                            // trusted: the server names these files, and a
                            // path outside the session's workspace must not
                            // ride out inside a preview.
                            .filter_map(|file| {
                                let path = workspace_relative_target(root, &file.file)?;
                                Some(LspRenameFileDto {
                                    path,
                                    edit_count: file.edits.len(),
                                })
                            })
                            .take(cap)
                            .collect();
                        if files.len() == cap {
                            truncated = true;
                        }
                        if preview.edit_count > codegg_protocol::lsp::MAX_LSP_RENAME_EDITS {
                            truncated = true;
                        }
                        // The per-file patches `egglsp` already computed are
                        // carried here instead of being discarded. A truncated
                        // preview, or a file whose patch was omitted, leaves
                        // the preview a preview.
                        let candidate = mint_rename_candidate(root, &preview, truncated);
                        LspReadPayloadDto::RenamePreview(LspRenamePreviewDto {
                            old_name: preview.old_name,
                            new_name: preview.new_name,
                            files,
                            edit_count: preview.edit_count,
                            warnings: preview
                                .warnings
                                .into_iter()
                                .take(codegg_protocol::lsp::MAX_LSP_PREVIEW_WARNINGS)
                                .collect(),
                            truncated,
                            base_stale: preview.base_stale,
                            candidate,
                        })
                    }
                    Err(err) => return Self::read_operation_error(operation, &err),
                }
            }
            LspReadOperation::CodeActionPreview => {
                // A code action is about a position, so this is a point read
                // at the cursor: a zero-width range there. The daemon asks
                // **once** and previews each action's own edit locally — see
                // `egglsp`'s `code_action_previews_for_warm_client` for why a
                // second request would be a hazard rather than a saving.
                let range = crate::lsp::lsp_types::Range {
                    start: crate::lsp::lsp_types::Position {
                        line,
                        character: column,
                    },
                    end: crate::lsp::lsp_types::Position {
                        line,
                        character: column,
                    },
                };
                match operations
                    .code_action_previews_for_warm_client(
                        key,
                        target,
                        range,
                        None,
                        // One extra, so `truncated` is a measured fact rather
                        // than a guess at the cap.
                        MAX_LSP_CODE_ACTIONS + 1,
                        Some(root),
                    )
                    .await
                {
                    Ok(mut entries) => {
                        let truncated = entries.len() > MAX_LSP_CODE_ACTIONS;
                        entries.truncate(MAX_LSP_CODE_ACTIONS);
                        let actions = mint_code_action_candidates(
                            root,
                            entries,
                            MAX_LSP_READ_CANDIDATE_BYTES,
                        );
                        LspReadPayloadDto::CodeActions { actions, truncated }
                    }
                    Err(err) => return Self::read_operation_error(operation, &err),
                }
            }
        };
        Ok(LspReadResultDto::ready(operation, payload))
    }

    /// Workspace symbols, still warm-only.
    ///
    /// Takes the already-resolved client so this path answers from the server
    /// the resolution chose, rather than from whichever client
    /// `first_client_key()` returns. In a polyglot workspace those differ, and
    /// the old routing meant the answer depended on hash-map order.
    async fn lsp_workspace_symbols(
        &self,
        service: &Arc<LspService>,
        root: &Path,
        operation: LspReadOperation,
        client: WarmClient,
        query: &str,
    ) -> Result<LspReadResultDto, LspReadDenial> {
        let key = match client {
            WarmClient::Cold => return Ok(LspReadResultDto::not_ready(operation)),
            WarmClient::Ambiguous { candidates } => {
                return Ok(LspReadResultDto::ambiguous(operation, candidates))
            }
            WarmClient::Resolved { key } => key,
        };

        let operations = crate::lsp::operations::LspOperations::new(service.clone());
        match operations
            .workspace_symbols_for_warm_client(&key, query)
            .await
        {
            Ok(symbols) => {
                let (symbols, truncated) = cap_symbols(
                    symbols
                        .into_iter()
                        .map(|symbol| symbol_information_to_dto(symbol, root)),
                );
                Ok(LspReadResultDto::ready(
                    operation,
                    LspReadPayloadDto::Symbols { symbols, truncated },
                ))
            }
            Err(_) => Err(LspReadDenial::ServerFailed),
        }
    }

    /// The authoritative current diagnostics for a project.
    ///
    /// This is the resync authority ADR-0012 §2 depends on. It reads the
    /// publisher's recorded sets, **not** `DiagnosticsCollector::
    /// get_diagnostics_for_file`: that accessor debounces and can return an
    /// *empty* set with `diagnostics_may_still_be_warming: false`, which as an
    /// authority would wipe a client's diagnostics on every reconciliation.
    ///
    /// Only files the daemon is already publishing for this project are
    /// reported. A file the publisher has not observed has no sequence, and
    /// inventing `0` would make "no data yet" indistinguishable from "a clean
    /// file with no diagnostics".
    async fn lsp_diagnostics_get(
        &self,
        request: LspDiagnosticsGetRequestDto,
    ) -> LspDiagnosticsResultDto {
        if request.project_id.is_empty() {
            return diagnostics_result(request.project_id, false, None, false);
        }
        // Gate on the service too: a daemon with no LSP cannot be
        // authoritative about diagnostics, and an empty set would read as
        // "this project is clean".
        if self.lsp_service().is_err() {
            return diagnostics_result(request.project_id, false, None, false);
        }
        let store = self.lsp_diagnostics.lock();
        let files = store.files_for(&request.project_id);
        let truncated = store.tracker_saturated(&request.project_id);
        diagnostics_result(request.project_id, true, files, truncated)
    }
}

fn diagnostics_result(
    project_id: String,
    service_available: bool,
    files: Option<Vec<codegg_protocol::lsp::LspFileDiagnosticsDto>>,
    truncated: bool,
) -> LspDiagnosticsResultDto {
    let (status, files, truncated) = match (service_available, files) {
        (true, Some(files)) => (
            codegg_protocol::lsp::LspDiagnosticsStatus::Ready,
            files,
            truncated,
        ),
        _ => (
            codegg_protocol::lsp::LspDiagnosticsStatus::NotReady,
            Vec::new(),
            false,
        ),
    };
    LspDiagnosticsResultDto {
        project_id,
        status,
        files,
        truncated,
    }
}

fn cap_locations<I: Iterator<Item = LspRangeDto>>(items: I) -> (Vec<LspRangeDto>, bool) {
    let cap = codegg_protocol::lsp::MAX_LSP_LOCATIONS;
    let mut collected: Vec<LspRangeDto> = items.take(cap + 1).collect();
    let truncated = collected.len() > cap;
    collected.truncate(cap);
    (collected, truncated)
}

fn cap_symbols<I: Iterator<Item = LspSymbolDto>>(items: I) -> (Vec<LspSymbolDto>, bool) {
    let cap = codegg_protocol::lsp::MAX_LSP_SYMBOLS;
    let mut collected: Vec<LspSymbolDto> = items.take(cap + 1).collect();
    let truncated = collected.len() > cap;
    collected.truncate(cap);
    (collected, truncated)
}

fn range_to_dto(range: crate::lsp::lsp_types::Range) -> LspRangeDto {
    LspRangeDto {
        path: String::new(),
        workspace_relative_path: None,
        start_line: range.start.line,
        start_column: range.start.character,
        end_line: range.end.line,
        end_column: range.end.character,
    }
}

/// Convert a `Location` to a DTO, keeping only its file name.
///
/// A full workspace-relative path would be more useful, but a location can
/// point outside the requesting session's project — `find_references`
/// legitimately returns hits in other crates — and relativizing against the
/// wrong root would fabricate a path that does not exist. The file name is
/// what can be reported without inventing authority over another root.
fn location_to_dto(location: &crate::lsp::lsp_types::Location, root: &Path) -> LspRangeDto {
    let mut dto = range_to_dto(location.range);
    dto.path = uri_file_name(&location.uri);
    dto.workspace_relative_path = uri_workspace_relative_path(&location.uri, root);
    dto
}

fn location_link_to_dto(link: &crate::lsp::lsp_types::LocationLink, root: &Path) -> LspRangeDto {
    let mut dto = range_to_dto(link.target_selection_range);
    dto.path = uri_file_name(&link.target_uri);
    dto.workspace_relative_path = uri_workspace_relative_path(&link.target_uri, root);
    dto
}

fn document_symbol_to_dto(symbol: crate::lsp::lsp_types::DocumentSymbol) -> LspSymbolDto {
    LspSymbolDto {
        name: symbol.name,
        kind: symbol_kind_number(symbol.kind),
        // `detail` is the server's own one-line qualifier. The nesting
        // hierarchy lives in `children`, which is not flattened here: a
        // bounded read reports the top level, and flattening would multiply
        // the payload without a bound the caller asked for.
        container: symbol.detail,
        range: range_to_dto(symbol.selection_range),
    }
}

fn symbol_information_to_dto(
    symbol: crate::lsp::lsp_types::SymbolInformation,
    root: &Path,
) -> LspSymbolDto {
    LspSymbolDto {
        name: symbol.name,
        kind: symbol_kind_number(symbol.kind),
        container: None,
        range: {
            let mut range = range_to_dto(symbol.location.range);
            range.path = uri_file_name(&symbol.location.uri);
            range.workspace_relative_path = uri_workspace_relative_path(&symbol.location.uri, root);
            range
        },
    }
}

fn uri_workspace_relative_path(uri: &crate::lsp::lsp_types::Uri, root: &Path) -> Option<String> {
    let target = url::Url::parse(uri.as_str()).ok()?.to_file_path().ok()?;
    workspace_relative_target(root, &target)
}

fn workspace_relative_target(root: &Path, target: &Path) -> Option<String> {
    let canonical_root = std::fs::canonicalize(root).ok()?;
    let canonical_target = std::fs::canonicalize(target).ok()?;
    let relative = canonical_target.strip_prefix(canonical_root).ok()?;
    let mut components = Vec::new();
    for component in relative.components() {
        let std::path::Component::Normal(part) = component else {
            return None;
        };
        components.push(part.to_str()?);
    }
    if components.is_empty() {
        return None;
    }
    Some(components.join("/"))
}

/// The daemon's candidate counter.
///
/// A candidate's revision says which version of a preview it is, and the
/// apply path rejects a zero revision, so the counter starts at one.
static PREVIEW_CANDIDATE_REVISION: AtomicU64 = AtomicU64::new(1);

/// Mint one reviewable candidate, or none.
///
/// The daemon owns the language server, so it owns the material the apply path
/// consumes: the patches, the base each was computed against, and the digest
/// that binds a request to exactly this candidate.
///
/// **Nothing is registered.** The apply path verifies a digest recomputed over
/// the request's *own* patches (`src/lsp/mutation.rs:380-401`), so a candidate
/// that travelled from here and was altered in transit fails the check. That
/// is what lets the candidate ride in a read result instead of living in a
/// registry — and it is what makes the bridge work in socket mode, where the
/// frontend has no LSP tool at all.
///
/// `None` means "this preview is not a candidate": it carries no patch, or the
/// material exceeds a bound the apply path would refuse anyway. The preview is
/// still a preview.
fn mint_preview_candidate(
    kind: &str,
    title: &str,
    provenance: &str,
    patches: Vec<(String, String, String)>,
) -> Option<LspPreviewCandidateDto> {
    if patches.is_empty() {
        return None;
    }
    // The digest is computed over exactly the material the request will
    // carry: the same paths, in the same order, the same patch text and the
    // same base hashes.
    let edits: Vec<String> = patches.iter().map(|(path, _, _)| path.clone()).collect();
    let mut hashes: HashMap<String, String> = HashMap::new();
    let mut patch_views: Vec<egglsp::PreviewFilePatch> = Vec::with_capacity(patches.len());
    for (path, patch, original_hash) in patches {
        hashes.insert(path.clone(), original_hash.clone());
        patch_views.push(egglsp::PreviewFilePatch {
            path,
            patch,
            original_hash,
        });
    }
    let preview_digest = egglsp::preview_registry::preview_digest_for_candidate(
        kind,
        title,
        provenance,
        &edits,
        &hashes,
        &patch_views,
    );
    let preview_revision = PREVIEW_CANDIDATE_REVISION.fetch_add(1, Ordering::Relaxed);
    let candidate = LspPreviewCandidateDto {
        preview_id: format!("editor-preview-{preview_revision}"),
        preview_revision,
        preview_digest,
        kind: kind.to_string(),
        title: title.to_string(),
        provenance: provenance.to_string(),
        patches: patch_views
            .into_iter()
            .map(|patch| LspPreviewPatchDto {
                path: patch.path,
                patch: patch.patch,
                original_hash: patch.original_hash,
            })
            .collect(),
    };
    // The bounds are the apply path's own, so a candidate that passes is one
    // the daemon can at least shape into a request. The daemon still decides
    // whether the change may be applied.
    candidate.validate().ok()?;
    Some(candidate)
}

/// Mint the candidate for a rename preview, or none.
///
/// The per-file patches `egglsp` computed for the preview are carried here
/// instead of being discarded — they were always the material the apply path
/// needed. A truncated preview, or a file whose patch was omitted, leaves the
/// preview a preview: a partial rename is not a rename.
fn mint_rename_candidate(
    root: &Path,
    preview: &egglsp::RenamePreview,
    truncated: bool,
) -> Option<LspPreviewCandidateDto> {
    if truncated {
        return None;
    }
    let mut patches = Vec::new();
    for file in &preview.affected_files {
        if file.patch_omitted || file.patch.is_empty() {
            continue;
        }
        let Some(path) = workspace_relative_target(root, &file.file) else {
            continue;
        };
        patches.push((path, file.patch.clone(), file.original_hash.clone()));
    }
    if patches.is_empty() {
        return None;
    }
    let old_name = preview.old_name.as_deref().unwrap_or("symbol");
    mint_preview_candidate(
        "rename",
        &format!("Rename {old_name} to {}", preview.new_name),
        &format!("editor:rename:{old_name}->{}", preview.new_name),
        patches,
    )
}

/// Mint the candidates for one code-action read, or the reason each action
/// has none.
///
/// The reasons are **per action**, not per result, because the causes are
/// per action: one action can be command-only while the one beside it is
/// perfectly applicable. A result-wide flag could only say "something was
/// cut", and the reader would not know which.
///
/// The order of the checks is the order a reader needs: the server's own
/// facts first (a command, a disabled marker), then the facts of the edit
/// (none, incomplete), and only then this surface's own bound. Each action
/// carries the most specific true statement about why it has no candidate.
fn mint_code_action_candidates(
    root: &Path,
    entries: Vec<egglsp::CodeActionEntry>,
    max_total_bytes: usize,
) -> Vec<LspCodeActionDto> {
    let mut actions = Vec::with_capacity(entries.len());
    // The running total of patch bytes this result carries. The bound is on
    // the *result*, not on each candidate: a read result travels over the
    // wire, and several candidates in one result would each be within their
    // own bound while the result as a whole was not.
    let mut total_bytes = 0usize;
    let mut bound_reached = false;
    for entry in entries {
        let mut candidate = None;
        let mut unavailable = None;
        if entry.has_command {
            // The surface never runs commands, and the LSP specification has
            // the client apply an action's edit *and then* its command.
            // Applying the edit of a mixed action would apply half of what
            // the action means.
            unavailable = Some(LspCodeActionUnavailable::Command);
        } else if entry.disabled_reason.is_some() {
            unavailable = Some(LspCodeActionUnavailable::Disabled);
        } else if !entry.has_edit {
            unavailable = Some(LspCodeActionUnavailable::NoEdit);
        } else {
            match code_action_patches(root, &entry) {
                None => unavailable = Some(LspCodeActionUnavailable::IncompletePreview),
                Some(patches) => {
                    let bytes: usize = patches.iter().map(|(_, patch, _)| patch.len()).sum();
                    if code_action_exceeds_candidate_bounds(&patches) {
                        unavailable = Some(LspCodeActionUnavailable::TooLarge);
                    } else if bound_reached || total_bytes.saturating_add(bytes) > max_total_bytes {
                        // The bound is reached: no further candidate is
                        // minted. The action keeps its place in the list, and
                        // says why it has no preview.
                        bound_reached = true;
                        unavailable = Some(LspCodeActionUnavailable::BoundReached);
                    } else {
                        total_bytes += bytes;
                        candidate = mint_preview_candidate(
                            "code_action",
                            &entry.title,
                            &format!("editor:code_action:{}", entry.title),
                            patches,
                        );
                        if candidate.is_none() {
                            // The mint refused it. The size bounds were
                            // checked above, so what is left is a material
                            // the apply path would refuse for its shape.
                            unavailable = Some(LspCodeActionUnavailable::IncompletePreview);
                        }
                    }
                }
            }
        }
        actions.push(LspCodeActionDto {
            title: entry.title,
            kind: entry.kind,
            preferred: entry.preferred,
            disabled_reason: entry.disabled_reason,
            has_edit: entry.has_edit,
            has_command: entry.has_command,
            candidate,
            unavailable,
        });
    }
    actions
}

/// The patches a code action's preview would apply, or `None` when the
/// preview is not complete enough to be one.
///
/// A truncated preview, a file whose patch was omitted, and a file outside
/// the workspace all mean the same thing here: the change is not fully
/// known, and a partial change is not a change the apply path can make.
fn code_action_patches(
    root: &Path,
    entry: &egglsp::CodeActionEntry,
) -> Option<Vec<(String, String, String)>> {
    let preview = entry.preview.as_ref()?;
    if preview.truncated {
        return None;
    }
    let mut patches = Vec::new();
    for file in &preview.affected_files {
        if file.patch_omitted || file.patch.is_empty() {
            return None;
        }
        // Containment is re-applied here rather than trusted: the server
        // names these files, and a path outside the session's workspace must
        // not ride out inside a preview.
        let path = workspace_relative_target(root, &file.file)?;
        patches.push((path, file.patch.clone(), file.original_hash.clone()));
    }
    if patches.is_empty() {
        return None;
    }
    Some(patches)
}

/// Whether the apply path would refuse this material for its size.
///
/// The bounds are the candidate's own, so a candidate this accepts is one
/// the daemon can at least shape into a request.
fn code_action_exceeds_candidate_bounds(patches: &[(String, String, String)]) -> bool {
    if patches.len() > MAX_LSP_PREVIEW_CANDIDATE_PATCHES {
        return true;
    }
    if patches
        .iter()
        .any(|(_, patch, _)| patch.len() > MAX_LSP_PREVIEW_CANDIDATE_PATCH_BYTES)
    {
        return true;
    }
    let total: usize = patches.iter().map(|(_, patch, _)| patch.len()).sum();
    total > MAX_LSP_PREVIEW_CANDIDATE_BYTES
}

fn uri_file_name(uri: &crate::lsp::lsp_types::Uri) -> String {
    // Parsed the same way `egglsp::client` parses a URI, so percent-decoding
    // behaves identically on both sides.
    url::Url::parse(uri.as_str())
        .ok()
        .and_then(|parsed| parsed.to_file_path().ok())
        .and_then(|path| {
            path.file_name()
                .map(|name| name.to_string_lossy().into_owned())
        })
        .unwrap_or_default()
}

/// The server's own numeric `SymbolKind`.
///
/// `SymbolKind` is `#[serde(transparent)]` over its `i32`, so the wire value
/// is read directly rather than matched against a 26-arm table. A table here
/// would make this protocol own a symbol taxonomy it does not define, and
/// would silently mis-report any kind a future LSP revision adds.
fn symbol_kind_number(kind: crate::lsp::lsp_types::SymbolKind) -> u32 {
    serde_json::to_value(kind)
        .ok()
        .and_then(|value| value.as_u64())
        .and_then(|number| u32::try_from(number).ok())
        .unwrap_or_default()
}

/// The server's own numeric `DocumentHighlightKind` (1 = text, 2 = read,
/// 3 = write).
///
/// Kept out of the protocol's vocabulary for the same reason as
/// [`symbol_kind_number`]: the number is the server's classification, and
/// relabelling it here would make this protocol own a taxonomy it does not
/// define. `None` stays `None` — "the server named no kind" is not "text".
fn highlight_kind_number(kind: crate::lsp::lsp_types::DocumentHighlightKind) -> u32 {
    serde_json::to_value(kind)
        .ok()
        .and_then(|value| value.as_u64())
        .and_then(|number| u32::try_from(number).ok())
        .unwrap_or_default()
}

// ---------------------------------------------------------------------------
// Publication
// ---------------------------------------------------------------------------

/// How often the daemon re-reads diagnostics looking for a change.
///
/// Deliberately not tight. The underlying data is push-based — the language
/// server sends `textDocument/publishDiagnostics` and egglsp caches it — so
/// this only has to notice a change that already happened. Polling faster
/// would burn wakeups to shorten a delay an editor does not notice.
pub const DIAGNOSTICS_POLL_INTERVAL: std::time::Duration = std::time::Duration::from_millis(750);

/// Read every warm client's diagnostics and publish the ones that changed.
///
/// Returns how many envelopes were published, so a caller — and a test — can
/// tell "nothing changed" from "the publisher did not run".
///
/// Three properties are load-bearing:
///
/// - It walks [`LspService::existing_client_keys_for_root`] for one `root`,
///   which lists **existing** clients only. Nothing here can start a language
///   server, matching the warm-only rule the read path enforces. It is scoped
///   to that root: the publisher used to walk every client in the process, so
///   with two projects open each project's tracker recorded the *other*
///   project's files, and every call re-walked every client.
/// - It publishes only on a digest change. An unchanged file produces no
///   envelope, which is what keeps a high-churn project from turning the
///   stream into a busy loop.
pub async fn publish_changed_diagnostics(
    daemon: &CoreDaemon,
    project_id: &str,
    session_id: &str,
    root: &Path,
) -> usize {
    use crate::core::lsp_diagnostics_store::DiagnosticsChange;

    let Some(service) = daemon.deps.lsp_service.clone() else {
        return 0;
    };

    let mut published = 0usize;
    for key in service.existing_client_keys_for_root(root).await {
        let Ok(per_file) = service.get_all_diagnostics_for_key(&key).await else {
            continue;
        };
        for (uri, diagnostics) in per_file {
            let Some(relative) = diagnostics_relative_path(&uri, root) else {
                continue;
            };
            let mapped = map_diagnostics(&diagnostics);
            let payload = codegg_protocol::lsp::LspFileDiagnosticsDto {
                path: relative.clone(),
                // Both filled in by the tracker, which owns the digest and the
                // sequence. They are not caller's to get wrong.
                sequence: 0,
                digest: String::new(),
                truncated: diagnostics.len() > codegg_protocol::lsp::MAX_LSP_DIAGNOSTICS_PER_FILE,
                diagnostics: mapped,
                post_restart: false,
            };

            let change = {
                let mut store = daemon.lsp_diagnostics.lock();
                store.tracker_mut(project_id).observe(&relative, payload)
            };
            let Some(change) = change else {
                // The tracked-file cap refused this file. That is reported
                // through the resync payload's `truncated` flag rather than
                // dropped silently.
                continue;
            };
            if matches!(change, DiagnosticsChange::Unchanged) {
                continue;
            }

            let recorded = {
                let mut store = daemon.lsp_diagnostics.lock();
                store.tracker_mut(project_id).file(&relative).cloned()
            };
            let Some(recorded) = recorded else {
                continue;
            };
            // `publish` is async and must be awaited: dropping the future
            // would compile (with a warning) and silently publish nothing,
            // which would look exactly like "the language server stopped
            // reporting diagnostics".
            daemon
                .event_log
                .publish(
                    Some(session_id.to_string()),
                    None,
                    CoreEvent::LspDiagnosticsUpdated {
                        session_id: session_id.to_string(),
                        project_id: project_id.to_string(),
                        file: recorded,
                    },
                )
                .await;
            published += 1;
        }
    }
    published
}

/// The workspace-relative path a diagnostic URI belongs to.
///
/// Diagnostics are keyed by URI, but the protocol reports workspace-relative
/// paths so a client can join them to its own file list.
///
/// This resolves the **full** relative path, not the file name. It used to
/// keep only `path.file_name()`, so `src/lib.rs` and `tests/lib.rs` were the
/// same key and the second silently overwrote the first in the tracker — the
/// client was shown one file's diagnostics under the other file's name. A
/// URI outside `root` is skipped rather than reported under a fabricated
/// path, because a wrong-but-present path is worse for a client than an
/// absent one.
fn diagnostics_relative_path(uri: &str, root: &Path) -> Option<String> {
    url::Url::parse(uri)
        .ok()
        .and_then(|parsed| parsed.to_file_path().ok())
        .and_then(|path| workspace_relative_target(root, &path))
}

fn map_diagnostics(
    diagnostics: &[crate::lsp::lsp_types::Diagnostic],
) -> Vec<codegg_protocol::lsp::LspDiagnosticDto> {
    use crate::lsp::lsp_types::DiagnosticTag;

    diagnostics
        .iter()
        .take(codegg_protocol::lsp::MAX_LSP_DIAGNOSTICS_PER_FILE)
        .map(|entry| {
            // Severity and tag are newtypes over their wire numbers; read the
            // server's own value rather than matching a table this protocol
            // does not define.
            let severity = entry
                .severity
                .and_then(|value| serde_json::to_value(value).ok())
                .and_then(|value| value.as_u64())
                .unwrap_or(0) as u32;
            let tag = entry
                .tags
                .as_ref()
                .and_then(|tags| tags.first().cloned())
                .map(|value| match value {
                    DiagnosticTag::UNNECESSARY => 1u32,
                    DiagnosticTag::DEPRECATED => 2u32,
                    _ => 0,
                })
                .unwrap_or(0);
            codegg_protocol::lsp::LspDiagnosticDto {
                range: codegg_protocol::lsp::LspRangeDto {
                    path: String::new(),
                    workspace_relative_path: None,
                    start_line: entry.range.start.line,
                    start_column: entry.range.start.character,
                    end_line: entry.range.end.line,
                    end_column: entry.range.end.character,
                },
                severity,
                tag,
                code: entry.code.as_ref().map(|value| match value {
                    crate::lsp::lsp_types::NumberOrString::Number(number) => number.to_string(),
                    crate::lsp::lsp_types::NumberOrString::String(text) => text.clone(),
                }),
                message: entry.message.clone(),
                source: entry.source.clone(),
            }
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Driver
// ---------------------------------------------------------------------------

/// One session the daemon may publish diagnostics for.
struct DiagnosticsTarget {
    project_id: String,
    session_id: String,
    root: PathBuf,
}

impl CoreDaemon {
    /// Start the daemon-owned diagnostics cadence.
    ///
    /// M013 found [`publish_changed_diagnostics`] with **zero callers**: the
    /// whole store, the digest tracker, the resync authority and this cadence
    /// existed, and nothing ever ran them, so `LspDiagnosticsUpdated` was
    /// never emitted and every client saw a permanently cold stream. The
    /// function was correct and unreachable.
    ///
    /// Why the daemon and not the frontend: ADR-0011 requires that a process
    /// with a workspace root may start a language server. A frontend-triggered
    /// poll would put that decision on whichever client happened to be open,
    /// and `NotReady` would then be indistinguishable from "this client
    /// never asked". Only a daemon-owned cadence has the workspace root for
    /// every live session regardless of who is connected.
    ///
    /// Why the [`SessionRuntimeRegistry`] rather than the session table: it
    /// holds sessions this daemon has actually resolved, so a session from a
    /// previous run — with a root that may since have been deleted — is not
    /// walked every 750 ms forever. It is the same registry the idle wake
    /// resolves through, so the cadence cannot outlive the sessions it serves.
    ///
    /// Holds a [`Weak`], so the task ends when the daemon does instead of
    /// keeping it alive.
    pub fn start_lsp_diagnostics_driver(self: &Arc<Self>) {
        let weak = Arc::downgrade(self);
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(DIAGNOSTICS_POLL_INTERVAL);
            // `interval` fires its first tick immediately. Discard it: at
            // construction no session is resolved yet, and an immediate pass
            // would walk an empty registry and look like a live one that did
            // nothing.
            interval.tick().await;
            loop {
                interval.tick().await;
                let Some(daemon) = weak.upgrade() else {
                    return;
                };
                daemon.publish_diagnostics_once().await;
            }
        });
    }

    /// One pass of the cadence.
    ///
    /// Separate from the loop so a test can drive exactly one pass instead of
    /// sleeping through a real interval — and so "the publisher is wired" is
    /// an assertion rather than a hope about timing.
    pub async fn publish_diagnostics_once(&self) {
        let Ok(service) = self.lsp_service() else {
            return;
        };
        for target in self.diagnostics_targets(&service).await {
            publish_changed_diagnostics(self, &target.project_id, &target.session_id, &target.root)
                .await;
        }
    }

    /// Live sessions whose workspace already has at least one warm client.
    ///
    /// The warm check is what keeps this cheap and honest: a session with no
    /// warm client has nothing to publish, and asking for its diagnostics
    /// would walk a client list that cannot contain it.
    async fn diagnostics_targets(&self, service: &Arc<LspService>) -> Vec<DiagnosticsTarget> {
        let mut targets = Vec::new();
        for session_id in self.sessions.list_sessions() {
            let Some(runtime) = self.sessions.get(&session_id) else {
                continue;
            };
            if runtime.project_id.is_empty() {
                continue;
            }
            // Several warm clients is not a reason to skip: each one can have
            // diagnostics, and the publisher records them per file. Only an
            // absence of warm clients means there is nothing to do.
            if service
                .existing_client_keys_for_root(&runtime.workspace_root)
                .await
                .is_empty()
            {
                continue;
            }
            targets.push(DiagnosticsTarget {
                project_id: runtime.project_id.clone(),
                session_id: runtime.session_id.clone(),
                root: runtime.workspace_root.clone(),
            });
        }
        targets
    }
}

// ---------------------------------------------------------------------------
// Gated subscription
// ---------------------------------------------------------------------------

impl CoreDaemon {
    /// Subscribe to a project's LSP diagnostics stream.
    ///
    /// The gate is `direct_project + file.read`, enforced by the same
    /// `authorize_request` preamble every other request gets — so this
    /// handler adds no authorization logic of its own. The gate lives in
    /// `operation_descriptor` precisely so that it cannot be forgotten here.
    ///
    /// This is a dedicated request rather than a flag on `ProjectionSubscribe`
    /// because the generic subscribe is `Opaque + project.observe`, which
    /// cannot express `file.read`. Reusing it would disclose source messages,
    /// code, and snippets to a principal that may only observe.
    pub(crate) async fn lsp_diagnostics_subscribe(
        &self,
        request: codegg_protocol::lsp::LspDiagnosticsSubscribeRequestDto,
        trusted_client_id: &str,
    ) -> Result<CoreResponse, AppError> {
        use codegg_protocol::projection::replay::{
            ProjectionCursor, ProjectionSnapshotBundle, ProjectionSubscriptionRequest,
        };

        let Some(ref seam) = self.projection_seam else {
            return Ok(CoreResponse::Error {
                code: "projection_unavailable".into(),
                message: "projection replay requires a SQLite-backed daemon".into(),
            });
        };
        if request.validate().is_err() {
            return Ok(CoreResponse::Error {
                code: "invalid_lsp_diagnostics_subscribe".into(),
                message: "the subscription request is not valid".into(),
            });
        }

        let subscription = ProjectionSubscriptionRequest {
            scope: codegg_protocol::projection::replay::ProjectionStreamKind::Project,
            scope_id: request.project_id.clone(),
            cursor: request.cursor,
            projection_version: request.projection_version,
        };
        let service = seam.service();
        let sub_id = match service
            .subscribe_project(&request.project_id, trusted_client_id, &subscription)
            .await
        {
            Ok(sub_id) => sub_id,
            Err(error) => {
                return Ok(CoreResponse::Error {
                    code: "lsp_diagnostics_subscribe_failed".into(),
                    message: error.to_string(),
                })
            }
        };

        let descriptor = match service
            .store()
            .get_or_create_project_stream(&request.project_id)
            .await
        {
            Ok((descriptor, _)) => descriptor,
            Err(error) => {
                // Never leave a subscription behind because the descriptor
                // could not be read; the same joined teardown the generic
                // path uses.
                if let Err(unsubscribe_error) = service.unsubscribe(&sub_id).await {
                    tracing::warn!(
                        error = %unsubscribe_error,
                        subscription_id = %sub_id.0,
                        "failed to clean up an LSP diagnostics subscription"
                    );
                }
                return Ok(CoreResponse::Error {
                    code: "projection_descriptor_missing".into(),
                    message: error.to_string(),
                });
            }
        };

        // The authoritative set travels with the subscription so a client
        // starts from the resync authority rather than from an empty view it
        // would then have to discover by polling.
        let snapshot = ProjectionSnapshotBundle::LspDiagnostics {
            project_id: request.project_id.clone(),
            files: self
                .lsp_diagnostics
                .lock()
                .files_for(&request.project_id)
                .unwrap_or_default(),
            truncated: self
                .lsp_diagnostics
                .lock()
                .tracker_saturated(&request.project_id),
        };
        let cursor = ProjectionCursor {
            stream_id: descriptor.stream_id.clone(),
            event_seq: descriptor.high_water_seq,
            projection_version: descriptor.projection_version,
        };
        // Read the floor before `descriptor` moves into the response.
        let retention_floor_seq = descriptor.retention_floor_seq;
        Ok(CoreResponse::ProjectionSubscribed {
            subscription_id: sub_id,
            descriptor,
            snapshot,
            cursor,
            retention_floor_seq,
        })
    }
}

#[cfg(test)]
mod diagnostics_readiness_tests {
    use super::*;

    #[test]
    fn workspace_relative_target_requires_canonical_containment() {
        let temp = tempfile::tempdir().expect("tempdir");
        let root = temp.path().join("workspace");
        let sibling = temp.path().join("workspace-sibling");
        std::fs::create_dir_all(root.join("src")).expect("workspace directory");
        std::fs::create_dir_all(&sibling).expect("sibling directory");
        let inside = root.join("src/lib.rs");
        let outside = sibling.join("secret.rs");
        std::fs::write(&inside, "inside").expect("inside file");
        std::fs::write(&outside, "outside").expect("outside file");

        assert_eq!(
            workspace_relative_target(&root, &inside),
            Some("src/lib.rs".to_owned())
        );
        assert_eq!(workspace_relative_target(&root, &outside), None);
    }

    #[test]
    fn uri_target_is_percent_decoded_only_after_containment() {
        let temp = tempfile::tempdir().expect("tempdir");
        let root = temp.path().join("workspace");
        std::fs::create_dir_all(&root).expect("workspace directory");
        let target = root.join("file name.rs");
        std::fs::write(&target, "source").expect("target file");
        let url = url::Url::from_file_path(&target).expect("file URL");
        let uri: crate::lsp::lsp_types::Uri = url.as_str().parse().expect("LSP URI");

        assert!(url.as_str().contains("%20"));
        assert_eq!(
            uri_workspace_relative_path(&uri, &root),
            Some("file name.rs".into())
        );
    }

    #[cfg(unix)]
    #[test]
    fn workspace_relative_target_rejects_symlink_escape() {
        use std::os::unix::fs::symlink;

        let temp = tempfile::tempdir().expect("tempdir");
        let root = temp.path().join("workspace");
        let outside = temp.path().join("outside.rs");
        std::fs::create_dir_all(&root).expect("workspace directory");
        std::fs::write(&outside, "outside").expect("outside file");
        symlink(&outside, root.join("escape.rs")).expect("symlink");

        assert_eq!(
            workspace_relative_target(&root, &root.join("escape.rs")),
            None
        );
    }

    #[test]
    fn unavailable_or_unobserved_diagnostics_are_not_ready() {
        for result in [
            diagnostics_result("project-1".into(), false, Some(Vec::new()), false),
            diagnostics_result("project-1".into(), true, None, false),
        ] {
            assert_eq!(
                result.status,
                codegg_protocol::lsp::LspDiagnosticsStatus::NotReady
            );
            assert!(result.files.is_empty());
            assert!(!result.truncated);
        }
    }

    #[test]
    fn an_observed_empty_set_is_ready_and_authoritative() {
        let result = diagnostics_result("project-1".into(), true, Some(Vec::new()), false);
        assert_eq!(
            result.status,
            codegg_protocol::lsp::LspDiagnosticsStatus::Ready
        );
        assert!(result.files.is_empty());
    }
}

/// The candidate the daemon mints for a preview.
///
/// The apply path recomputes the candidate's digest over the request's *own*
/// patches and refuses a mismatch. So the digest the daemon mints has to be
/// computable from the candidate's own fields — the same path strings, the
/// same patch text, the same base hashes. If the daemon ever computed the
/// digest over one set of strings and published another, every editor apply
/// would be refused by the daemon with no clue why.
#[cfg(test)]
mod preview_candidate_tests {
    use super::*;

    /// Recompute a candidate's digest from the candidate's own fields.
    fn recompute(candidate: &LspPreviewCandidateDto) -> String {
        let edits: Vec<String> = candidate.patches.iter().map(|p| p.path.clone()).collect();
        let hashes: HashMap<String, String> = candidate
            .patches
            .iter()
            .map(|p| (p.path.clone(), p.original_hash.clone()))
            .collect();
        let patch_views: Vec<egglsp::PreviewFilePatch> = candidate
            .patches
            .iter()
            .map(|p| egglsp::PreviewFilePatch {
                path: p.path.clone(),
                patch: p.patch.clone(),
                original_hash: p.original_hash.clone(),
            })
            .collect();
        egglsp::preview_registry::preview_digest_for_candidate(
            &candidate.kind,
            &candidate.title,
            &candidate.provenance,
            &edits,
            &hashes,
            &patch_views,
        )
    }

    #[test]
    fn a_minted_candidate_digest_is_recomputable_from_its_own_fields() {
        let candidate = mint_preview_candidate(
            "rename",
            "Rename old to new",
            "editor:rename:old->new",
            vec![(
                "src/lib.rs".to_string(),
                "@@ -1 +1 @@\n-old\n+new\n".to_string(),
                "a".repeat(64),
            )],
        )
        .expect("a candidate with a patch is mintable");

        assert_eq!(
            candidate.preview_digest,
            recompute(&candidate),
            "the apply path recomputes the digest from the request's own patches"
        );
        assert_eq!(candidate.preview_digest.len(), 64);
        assert_ne!(candidate.preview_revision, 0, "a zero revision is refused");
        assert!(!candidate.preview_id.is_empty());
        assert_eq!(candidate.validate(), Ok(()));
    }

    #[test]
    fn two_candidates_get_different_revisions() {
        let first = mint_preview_candidate(
            "formatting",
            "Format a",
            "editor:format:a",
            vec![("a.rs".to_string(), "p".to_string(), "a".repeat(64))],
        )
        .expect("mintable");
        let second = mint_preview_candidate(
            "formatting",
            "Format b",
            "editor:format:b",
            vec![("b.rs".to_string(), "p".to_string(), "a".repeat(64))],
        )
        .expect("mintable");
        assert_ne!(
            first.preview_revision, second.preview_revision,
            "two candidates must be distinguishable"
        );
        assert_ne!(first.preview_id, second.preview_id);
    }

    #[test]
    fn a_candidate_with_no_patch_is_not_minted() {
        assert!(
            mint_preview_candidate("formatting", "Format a", "editor:format:a", Vec::new())
                .is_none(),
            "a candidate with no patches changes nothing"
        );
    }

    /// A rename preview whose only file had its patch omitted cannot become a
    /// candidate: the change would be partial, and a partial rename is not a
    /// rename.
    #[test]
    fn a_rename_whose_only_patch_was_omitted_mints_no_candidate() {
        let dir = tempfile::tempdir().expect("temp dir");
        let file = dir.path().join("lib.rs");
        std::fs::write(&file, "fn main() {}\n").expect("write");
        let preview = egglsp::RenamePreview {
            old_name: Some("main".to_string()),
            new_name: "run".to_string(),
            affected_files: vec![egglsp::FileEditPreview {
                file: file.clone(),
                original_hash: "a".repeat(64),
                edits: Vec::new(),
                patch: String::new(),
                patch_omitted: true,
            }],
            edit_count: 1,
            warnings: Vec::new(),
            truncated: false,
            base_stale: false,
            affected_file_versions: Vec::new(),
            server_generation: 1,
        };
        assert!(
            mint_rename_candidate(dir.path(), &preview, false).is_none(),
            "a rename with no applicable patch must stay a preview"
        );
    }

    /// A truncated preview is not a candidate either, whatever it contains.
    #[test]
    fn a_truncated_rename_mints_no_candidate() {
        let dir = tempfile::tempdir().expect("temp dir");
        let file = dir.path().join("lib.rs");
        std::fs::write(&file, "fn main() {}\n").expect("write");
        let preview = egglsp::RenamePreview {
            old_name: Some("main".to_string()),
            new_name: "run".to_string(),
            affected_files: vec![egglsp::FileEditPreview {
                file: file.clone(),
                original_hash: "a".repeat(64),
                edits: Vec::new(),
                patch: "@@ -1 +1 @@\n-a\n+b\n".to_string(),
                patch_omitted: false,
            }],
            edit_count: 1,
            warnings: Vec::new(),
            truncated: true,
            base_stale: false,
            affected_file_versions: Vec::new(),
            server_generation: 1,
        };
        assert!(
            mint_rename_candidate(dir.path(), &preview, true).is_none(),
            "a truncated preview must stay a preview"
        );
    }

    /// The candidate's paths are workspace-relative and contained.
    #[test]
    fn a_minted_candidate_names_its_files_relative_to_the_workspace() {
        let dir = tempfile::tempdir().expect("temp dir");
        let nested = dir.path().join("src");
        std::fs::create_dir_all(&nested).expect("mkdir");
        let file = nested.join("lib.rs");
        std::fs::write(&file, "fn main() {}\n").expect("write");
        let preview = egglsp::RenamePreview {
            old_name: Some("main".to_string()),
            new_name: "run".to_string(),
            affected_files: vec![egglsp::FileEditPreview {
                file: file.clone(),
                original_hash: "a".repeat(64),
                edits: Vec::new(),
                patch: "@@ -1 +1 @@\n-a\n+b\n".to_string(),
                patch_omitted: false,
            }],
            edit_count: 1,
            warnings: Vec::new(),
            truncated: false,
            base_stale: false,
            affected_file_versions: Vec::new(),
            server_generation: 1,
        };
        let candidate = mint_rename_candidate(dir.path(), &preview, false)
            .expect("a contained rename is mintable");
        assert_eq!(candidate.patches.len(), 1);
        assert_eq!(
            candidate.patches[0].path, "src/lib.rs",
            "the path must be workspace-relative, so the apply path joins it to the root"
        );
        assert_eq!(candidate.preview_digest, recompute(&candidate));
    }

    /// One code action whose edit previews cleanly.
    fn code_action_entry(title: &str, file: &Path, patch: &str) -> egglsp::CodeActionEntry {
        egglsp::CodeActionEntry {
            title: title.to_string(),
            kind: Some("quickfix".to_string()),
            preferred: false,
            disabled_reason: None,
            has_edit: true,
            has_command: false,
            preview: Some(egglsp::CodeActionPreview {
                title: title.to_string(),
                kind: Some("quickfix".to_string()),
                affected_files: vec![egglsp::FileEditPreview {
                    file: file.to_path_buf(),
                    original_hash: "a".repeat(64),
                    edits: Vec::new(),
                    patch: patch.to_string(),
                    patch_omitted: false,
                }],
                edit_count: 1,
                warnings: Vec::new(),
                truncated: false,
                server_generation: 1,
            }),
        }
    }

    /// A workspace with one file, and the path to it.
    fn code_action_workspace() -> (tempfile::TempDir, std::path::PathBuf) {
        let dir = tempfile::tempdir().expect("temp dir");
        let file = dir.path().join("lib.rs");
        std::fs::write(&file, "fn main() {}\n").expect("write");
        (dir, file)
    }

    #[test]
    fn a_code_action_candidate_digest_is_recomputable_from_its_own_fields() {
        let (dir, file) = code_action_workspace();
        let actions = mint_code_action_candidates(
            dir.path(),
            vec![code_action_entry(
                "Add missing import",
                &file,
                "@@ -1 +1 @@\n-use a;\n+use b;\n",
            )],
            MAX_LSP_READ_CANDIDATE_BYTES,
        );
        assert_eq!(actions.len(), 1);
        let candidate = actions[0]
            .candidate
            .clone()
            .expect("an editable action with room is a candidate");
        assert_eq!(candidate.kind, "code_action");
        assert_eq!(candidate.title, "Add missing import");
        assert_eq!(
            candidate.patches[0].path, "lib.rs",
            "the path must be workspace-relative"
        );
        assert_eq!(
            candidate.preview_digest,
            recompute(&candidate),
            "the apply path recomputes the digest from the request's own patches"
        );
        assert_eq!(actions[0].unavailable, None);
    }

    #[test]
    fn a_command_bearing_action_gets_no_candidate() {
        // The surface never runs commands, and the LSP specification applies
        // an action's edit *and then* its command. An action that carries a
        // command is never applied here — not even one that also carries an
        // edit, which would be half of what the action means.
        let (dir, file) = code_action_workspace();
        let mut entry = code_action_entry("Run the tests", &file, "@@ -1 +1 @@\n-a\n+b\n");
        entry.has_command = true;
        let actions =
            mint_code_action_candidates(dir.path(), vec![entry], MAX_LSP_READ_CANDIDATE_BYTES);
        assert!(actions[0].candidate.is_none());
        assert_eq!(
            actions[0].unavailable,
            Some(LspCodeActionUnavailable::Command)
        );
    }

    #[test]
    fn a_disabled_action_gets_no_candidate_and_keeps_the_servers_reason() {
        let (dir, file) = code_action_workspace();
        let mut entry = code_action_entry("Refactor this", &file, "@@ -1 +1 @@\n-a\n+b\n");
        entry.disabled_reason = Some("the server says this is not applicable here".to_string());
        let actions =
            mint_code_action_candidates(dir.path(), vec![entry], MAX_LSP_READ_CANDIDATE_BYTES);
        assert!(actions[0].candidate.is_none());
        assert_eq!(
            actions[0].unavailable,
            Some(LspCodeActionUnavailable::Disabled)
        );
        assert_eq!(
            actions[0].disabled_reason.as_deref(),
            Some("the server says this is not applicable here"),
            "the server's own words are carried, not re-worded"
        );
    }

    #[test]
    fn an_action_with_no_edit_gets_no_candidate() {
        let (dir, _file) = code_action_workspace();
        let entry = egglsp::CodeActionEntry {
            title: "Do nothing".to_string(),
            kind: None,
            preferred: false,
            disabled_reason: None,
            has_edit: false,
            has_command: false,
            preview: None,
        };
        let actions =
            mint_code_action_candidates(dir.path(), vec![entry], MAX_LSP_READ_CANDIDATE_BYTES);
        assert!(actions[0].candidate.is_none());
        assert_eq!(
            actions[0].unavailable,
            Some(LspCodeActionUnavailable::NoEdit)
        );
    }

    #[test]
    fn the_daemon_stops_minting_when_the_result_reaches_its_bound() {
        // The bound is on the *result*, not on each candidate: a read result
        // travels over the wire, and several candidates in one result would
        // each be within their own bound while the result was not.
        let (dir, file) = code_action_workspace();
        let patch = "x".repeat(64);
        let entries: Vec<egglsp::CodeActionEntry> = (0..3)
            .map(|index| code_action_entry(&format!("Action {index}"), &file, &patch))
            .collect();
        // Room for one candidate's worth of patch bytes and no more.
        let actions = mint_code_action_candidates(dir.path(), entries, patch.len());
        assert!(
            actions[0].candidate.is_some(),
            "the first action fits in the bound"
        );
        assert!(
            actions[1].candidate.is_none(),
            "the second would exceed the bound, so no candidate is minted"
        );
        assert_eq!(
            actions[1].unavailable,
            Some(LspCodeActionUnavailable::BoundReached)
        );
        assert_eq!(
            actions[2].unavailable,
            Some(LspCodeActionUnavailable::BoundReached),
            "minting stops at the bound rather than skipping ahead"
        );
    }

    #[test]
    fn a_file_outside_the_workspace_leaves_the_action_without_a_candidate() {
        // Containment is re-applied rather than trusted: the server names the
        // files, and a path outside the session's workspace must not ride out
        // inside a preview.
        let dir = tempfile::tempdir().expect("temp dir");
        let outside = dir.path().join("outside.rs");
        std::fs::write(&outside, "outside\n").expect("write");
        let root = dir.path().join("workspace");
        std::fs::create_dir_all(&root).expect("mkdir");
        let entry = code_action_entry("Edit outside", &outside, "@@ -1 +1 @@\n-a\n+b\n");
        let actions = mint_code_action_candidates(&root, vec![entry], MAX_LSP_READ_CANDIDATE_BYTES);
        assert!(actions[0].candidate.is_none());
        assert_eq!(
            actions[0].unavailable,
            Some(LspCodeActionUnavailable::IncompletePreview)
        );
    }
}

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
//! - **Warm-only.** Every read first asks
//!   `find_existing_client_for_root_hint`, which resolves an existing client
//!   or errors and never falls through to a create path. A cold read returns
//!   `NotReady` and cannot spawn a language server.
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

use std::path::{Path, PathBuf};
use std::sync::Arc;

use codegg_protocol::lsp::{
    LspDiagnosticsGetRequestDto, LspDiagnosticsResultDto, LspRangeDto, LspReadOperation,
    LspReadPayloadDto, LspReadRequestDto, LspReadResultDto, LspSymbolDto,
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
            Self::OutsideWorkspace | Self::SessionNotFound => "the requested path is not available",
            Self::ServiceUnavailable => "LSP is unavailable for this request",
        }
    }

    pub fn code(self) -> &'static str {
        match self {
            Self::IncoherentRequest | Self::InvalidPath => "invalid_lsp_read",
            Self::OutsideWorkspace | Self::SessionNotFound => "lsp_read_not_found",
            Self::ServiceUnavailable => "lsp_unavailable",
        }
    }
}

impl CoreDaemon {
    /// Dispatch one M006-B LSP read request.
    pub(crate) async fn handle_lsp_request(
        &self,
        request: CoreRequest,
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
    async fn lsp_workspace_root(&self, session_id: &str) -> Result<PathBuf, LspReadDenial> {
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
    fn lsp_service(&self) -> Result<Arc<LspService>, LspReadDenial> {
        self.deps
            .lsp_service
            .clone()
            .ok_or(LspReadDenial::ServiceUnavailable)
    }

    /// Resolve a workspace-relative path against the daemon-owned root using
    /// the same containment primitive as the write path.
    fn resolve_target(root: &Path, relative: &str) -> Result<PathBuf, LspReadDenial> {
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
        if operation == LspReadOperation::WorkspaceSymbols {
            // Project-wide, so no path to resolve.
            let query = request.query.as_deref().unwrap_or_default();
            return self.lsp_workspace_symbols(&service, &root, query).await;
        }

        let target = Self::resolve_target(&root, &request.path)?;

        // Warm-only. This is the whole of ADR-0012 §3: with no client running
        // for this root the answer is `NotReady` — never a create, never an
        // empty success that would read as "clean".
        if service
            .find_existing_client_for_root_hint(Some(&root), None)
            .await
            .is_err()
        {
            return Ok(LspReadResultDto::not_ready(operation));
        }
        self.lsp_point_read(&service, operation, &target, &request)
            .await
    }

    async fn lsp_point_read(
        &self,
        service: &Arc<LspService>,
        operation: LspReadOperation,
        target: &Path,
        request: &LspReadRequestDto,
    ) -> Result<LspReadResultDto, LspReadDenial> {
        let operations = crate::lsp::operations::LspOperations::new(service.clone());
        let line = request.line.unwrap_or_default();
        let column = request.column.unwrap_or_default();

        let payload = match operation {
            LspReadOperation::Hover => match operations.hover(target, line, column).await {
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
                Err(_) => return Err(LspReadDenial::OutsideWorkspace),
            },
            LspReadOperation::Definition => {
                match operations.go_to_definition(target, line, column).await {
                    Ok(links) => {
                        let (locations, truncated) =
                            cap_locations(links.iter().map(location_link_to_dto));
                        LspReadPayloadDto::Locations {
                            locations,
                            truncated,
                        }
                    }
                    Err(_) => return Err(LspReadDenial::OutsideWorkspace),
                }
            }
            LspReadOperation::References => {
                match operations.find_references(target, line, column).await {
                    Ok(found) => {
                        let (locations, truncated) =
                            cap_locations(found.iter().map(location_to_dto));
                        LspReadPayloadDto::Locations {
                            locations,
                            truncated,
                        }
                    }
                    Err(_) => return Err(LspReadDenial::OutsideWorkspace),
                }
            }
            LspReadOperation::DocumentSymbols => match operations.document_symbols(target).await {
                Ok(symbols) => {
                    let (symbols, truncated) =
                        cap_symbols(symbols.into_iter().map(document_symbol_to_dto));
                    LspReadPayloadDto::Symbols { symbols, truncated }
                }
                Err(_) => return Err(LspReadDenial::OutsideWorkspace),
            },
            LspReadOperation::WorkspaceSymbols => {
                unreachable!("routed by the caller before reaching a point read")
            }
            LspReadOperation::SemanticTokens => {
                let cap = codegg_protocol::lsp::MAX_LSP_SEMANTIC_TOKENS;
                match operations.semantic_tokens(target, cap + 1).await {
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
                    Err(_) => return Err(LspReadDenial::OutsideWorkspace),
                }
            }
        };
        Ok(LspReadResultDto::ready(operation, payload))
    }

    /// Workspace symbols, still warm-only.
    ///
    /// `workspace_symbols` uses `first_client_key()` and already fails with
    /// `NotInitialized` rather than launching, so this warm check is a
    /// root-scoped equivalent rather than a second launch guard.
    async fn lsp_workspace_symbols(
        &self,
        service: &Arc<LspService>,
        root: &Path,
        query: &str,
    ) -> Result<LspReadResultDto, LspReadDenial> {
        if service
            .find_existing_client_for_root_hint(Some(root), None)
            .await
            .is_err()
        {
            return Ok(LspReadResultDto::not_ready(
                LspReadOperation::WorkspaceSymbols,
            ));
        }
        let operations = crate::lsp::operations::LspOperations::new(service.clone());
        match operations.workspace_symbols(query).await {
            Ok(symbols) => {
                let (symbols, truncated) =
                    cap_symbols(symbols.into_iter().map(symbol_information_to_dto));
                Ok(LspReadResultDto::ready(
                    LspReadOperation::WorkspaceSymbols,
                    LspReadPayloadDto::Symbols { symbols, truncated },
                ))
            }
            Err(_) => Err(LspReadDenial::OutsideWorkspace),
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
        let empty = LspDiagnosticsResultDto {
            project_id: request.project_id.clone(),
            files: Vec::new(),
            truncated: false,
        };
        if request.project_id.is_empty() {
            return empty;
        }
        // Gate on the service too: a daemon with no LSP cannot be
        // authoritative about diagnostics, and an empty set would read as
        // "this project is clean".
        if self.lsp_service().is_err() {
            return empty;
        }
        self.lsp_diagnostics
            .lock()
            .files_for(&request.project_id)
            .map(|files| LspDiagnosticsResultDto {
                project_id: request.project_id.clone(),
                files,
                truncated: self
                    .lsp_diagnostics
                    .lock()
                    .tracker_saturated(&request.project_id),
            })
            .unwrap_or(empty)
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
fn location_to_dto(location: &crate::lsp::lsp_types::Location) -> LspRangeDto {
    let mut dto = range_to_dto(location.range);
    dto.path = uri_file_name(&location.uri);
    dto
}

fn location_link_to_dto(link: &crate::lsp::lsp_types::LocationLink) -> LspRangeDto {
    let mut dto = range_to_dto(link.target_selection_range);
    dto.path = uri_file_name(&link.target_uri);
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

fn symbol_information_to_dto(symbol: crate::lsp::lsp_types::SymbolInformation) -> LspSymbolDto {
    LspSymbolDto {
        name: symbol.name,
        kind: symbol_kind_number(symbol.kind),
        container: None,
        range: range_to_dto(symbol.location.range),
    }
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
/// Two properties are load-bearing:
///
/// - It walks `client_keys()`, which lists **existing** clients only. Nothing
///   here can start a language server, matching the warm-only rule the read
///   path enforces.
/// - It publishes only on a digest change. An unchanged file produces no
///   envelope, which is what keeps a high-churn project from turning the
///   stream into a busy loop.
pub async fn publish_changed_diagnostics(
    daemon: &CoreDaemon,
    project_id: &str,
    session_id: &str,
) -> usize {
    use crate::core::lsp_diagnostics_store::DiagnosticsChange;

    let Some(service) = daemon.deps.lsp_service.clone() else {
        return 0;
    };

    let mut published = 0usize;
    for key in service.client_keys().await {
        let Ok(per_file) = service.get_all_diagnostics_for_key(&key).await else {
            continue;
        };
        for (uri, diagnostics) in per_file {
            let Some(relative) = diagnostics_relative_path(&uri) else {
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

/// The file name a diagnostic URI belongs to.
///
/// Diagnostics are keyed by URI, but the protocol reports workspace-relative
/// paths so a client can join them to its own file list. A URI that will not
/// convert is skipped rather than reported under a fabricated path, because a
/// wrong-but-present path is worse for a client than an absent one.
fn diagnostics_relative_path(uri: &str) -> Option<String> {
    url::Url::parse(uri)
        .ok()
        .and_then(|parsed| parsed.to_file_path().ok())
        .and_then(|path| {
            path.file_name()
                .map(|name| name.to_string_lossy().into_owned())
        })
        .filter(|name| !name.is_empty())
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

use std::path::Path;

use lsp_types::*;
use sha2::{Digest, Sha256};
use similar::TextDiff;
use tracing::trace;
use url::Url;

use crate::capability::{LspCapabilitySnapshot, SemanticTokenLegendSnapshot};
use crate::client::url_to_uri;
use crate::edit::{
    preview_text_edits_for_file, preview_workspace_edit, validate_path_against_root,
    FileEditPreview, WorkspaceEditPreview,
};
use crate::error::LspError;
use crate::language::detect_language;
use crate::overlay::{
    diagnostic_to_file_diagnostic, flatten_symbols, OverlaySession, SemanticCheckPreview,
};
use crate::service::LspService;
use crate::LspSemanticOperation;

/// Default cap on signature-help / signature documentation strings.
///
/// The LSP spec allows documentation to be a full Markdown blob; for
/// tool output we bound it so a misbehaving server cannot blow up the
/// payload size.
pub const SIGNATURE_DOC_MAX_CHARS: usize = 2000;

/// Compact signature help DTO returned to model-facing surfaces.
///
/// Truncates documentation strings to [`SIGNATURE_DOC_MAX_CHARS`]
/// characters per item. Parameter offsets (`[start, end]` ranges into
/// the signature label) are resolved to substrings of `label`, matching
/// the behavior of [`format_signature_help`].
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct SignatureHelpSummary {
    pub active_signature: Option<u32>,
    pub active_parameter: Option<u32>,
    pub signatures: Vec<SignatureInfoSummary>,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct SignatureInfoSummary {
    pub label: String,
    pub documentation: Option<String>,
    pub parameters: Vec<SignatureParameterSummary>,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct SignatureParameterSummary {
    pub label: String,
    pub documentation: Option<String>,
}

impl SignatureHelpSummary {
    /// Build a normalized summary from a raw `SignatureHelp`. Returns
    /// `None` when `help` has no signatures.
    pub fn from_signature_help(help: &SignatureHelp) -> Option<Self> {
        if help.signatures.is_empty() {
            return None;
        }
        let signatures = help
            .signatures
            .iter()
            .map(|sig| SignatureInfoSummary {
                label: sig.label.clone(),
                documentation: sig.documentation.as_ref().map(format_documentation_clamped),
                parameters: sig
                    .parameters
                    .as_ref()
                    .map(|params| {
                        params
                            .iter()
                            .map(|p| SignatureParameterSummary {
                                label: resolve_parameter_label(&sig.label, &p.label),
                                documentation: p
                                    .documentation
                                    .as_ref()
                                    .map(format_documentation_clamped),
                            })
                            .collect()
                    })
                    .unwrap_or_default(),
            })
            .collect();
        Some(Self {
            active_signature: help.active_signature,
            active_parameter: help.active_parameter,
            signatures,
        })
    }
}

/// Truncate a documentation string to [`SIGNATURE_DOC_MAX_CHARS`].
pub fn truncate_doc(input: &str, max: usize) -> String {
    if input.len() <= max {
        return input.to_string();
    }
    // Walk char boundaries so we never split a UTF-8 codepoint.
    let mut end = max;
    while end > 0 && !input.is_char_boundary(end) {
        end -= 1;
    }
    let mut out = String::with_capacity(end + 16);
    out.push_str(&input[..end]);
    out.push('…');
    out
}

fn format_documentation_clamped(doc: &Documentation) -> String {
    let raw = format_documentation(doc);
    truncate_doc(&raw, SIGNATURE_DOC_MAX_CHARS)
}

fn resolve_parameter_label(sig_label: &str, label: &ParameterLabel) -> String {
    match label {
        ParameterLabel::Simple(s) => s.clone(),
        ParameterLabel::LabelOffsets([start, end]) => {
            let s = *start as usize;
            let e = *end as usize;
            if e <= sig_label.len() && s <= e {
                sig_label[s..e].to_string()
            } else {
                String::new()
            }
        }
    }
}

// ── Phase 4 Pass 6/7/8 DTOs ────────────────────────────────────────

/// Default cap on the number of files reported in a [`RenamePreview`].
pub const RENAME_PREVIEW_MAX_FILES: usize = 100;

/// Default cap on the number of edits reported in a [`RenamePreview`].
pub const RENAME_PREVIEW_MAX_EDITS: usize = 1000;

/// Default cap on the per-action unified diff inside
/// [`FormattingPreview`]. The diff is truncated to this many bytes
/// when the formatted content would produce a larger patch.
pub const FORMATTING_PREVIEW_MAX_DIFF_BYTES: usize = 8 * 1024;

/// Default cap on the number of [`CodeActionSummary`] entries
/// returned by [`LspOperations::code_action_summaries`].
pub const CODE_ACTION_SUMMARY_DEFAULT_MAX: usize = 50;

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
    /// selection) starting at this range.
    DefaultBehavior { range: lsp_types::Range },
    /// Server does not advertise prepare-rename support.
    Unavailable(crate::capability::LspUnavailable),
}

impl PrepareRenameResult {
    /// Build a typed result from a raw `PrepareRenameResponse`.
    pub fn from_response(resp: Option<PrepareRenameResponse>) -> Self {
        match resp {
            None => PrepareRenameResult::DefaultBehavior {
                range: lsp_types::Range::default(),
            },
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
            Some(PrepareRenameResponse::DefaultBehavior {
                default_behavior: _,
            }) => PrepareRenameResult::DefaultBehavior {
                range: lsp_types::Range::default(),
            },
        }
    }

    /// The range over which a rename would apply (best-effort; for
    /// `DefaultBehavior` the range is empty because the server did
    /// not commit to one).
    pub fn range(&self) -> Option<&lsp_types::Range> {
        match self {
            Self::Range { range, .. } => Some(range),
            Self::DefaultBehavior { range } => {
                if range == &lsp_types::Range::default() {
                    None
                } else {
                    Some(range)
                }
            }
            Self::Unavailable(_) => None,
        }
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
    pub affected_files: Vec<FileEditPreview>,
    /// Total number of text edits across all files. Capped at
    /// [`RENAME_PREVIEW_MAX_EDITS`]; see `truncated` for overflow.
    pub edit_count: usize,
    /// Structured warnings (e.g. resource operations present in
    /// the raw edit that the preview pipeline could not surface).
    pub warnings: Vec<String>,
    /// True when the underlying server's edit count or file count
    /// exceeded the preview caps and was clamped.
    pub truncated: bool,
    /// Authoritative server generation of the live client.
    pub server_generation: u64,
}

/// Bounded, preview-only code-action summary DTO. The raw
/// `WorkspaceEdit` and `Command` payloads are intentionally not
/// exposed — this surface is read-only and never applies edits.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct CodeActionSummary {
    pub title: String,
    /// LSP `CodeActionKind` (e.g. `"quickfix"`,
    /// `"refactor.extract"`, `"source.organizeImports"`). `None`
    /// for raw `Command` actions and for `CodeAction` values
    /// without a kind.
    pub kind: Option<String>,
    pub preferred: bool,
    /// `disabled.reason` from the LSP `CodeAction.disabled` field.
    pub disabled_reason: Option<String>,
    /// True when the action carries a `WorkspaceEdit` payload.
    pub has_edit: bool,
    /// True when the action carries a `Command` payload. The
    /// surface never executes commands; `has_command == true`
    /// indicates the action is command-only and cannot be
    /// previewed.
    pub has_command: bool,
    /// Bounded diagnostic descriptions (code + message) this
    /// action is reported to address. Server order is preserved.
    pub diagnostics: Vec<String>,
}

impl CodeActionSummary {
    /// Build a summary from a single raw `CodeActionOrCommand`.
    /// Pure conversion — does not touch the network.
    pub fn from_action(action: &CodeActionOrCommand) -> Self {
        match action {
            CodeActionOrCommand::Command(cmd) => Self {
                title: cmd.title.clone(),
                kind: None,
                preferred: false,
                disabled_reason: None,
                has_edit: false,
                has_command: true,
                diagnostics: Vec::new(),
            },
            CodeActionOrCommand::CodeAction(ca) => Self {
                title: ca.title.clone(),
                kind: ca.kind.as_ref().map(|k| k.as_str().to_string()),
                preferred: ca.is_preferred.unwrap_or(false),
                disabled_reason: ca.disabled.as_ref().map(|d| d.reason.clone()),
                has_edit: ca.edit.is_some(),
                has_command: ca.command.is_some(),
                diagnostics: ca
                    .diagnostics
                    .as_ref()
                    .map(|ds| {
                        ds.iter()
                            .map(|d| format!("{:?}: {}", d.code, d.message))
                            .collect()
                    })
                    .unwrap_or_default(),
            },
        }
    }
}

/// Bounded, preview-only code-action DTO. Wraps a
/// [`WorkspaceEditPreview`] (built from the resolved action's
/// `WorkspaceEdit`) with structured warnings. Commands are
/// rejected up-front (they cannot be previewed) and surface as
/// `LspError::CommandOnlyCodeAction`.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct CodeActionPreview {
    pub title: String,
    pub kind: Option<String>,
    /// Per-file preview entries. Out-of-root files produced
    /// errors during the underlying `preview_workspace_edit` call
    /// and are not present here.
    pub affected_files: Vec<FileEditPreview>,
    pub edit_count: usize,
    /// Structured warnings (e.g. resource operations present in
    /// the raw edit that the preview pipeline could not surface).
    pub warnings: Vec<String>,
    /// True when the underlying edit count or file count
    /// exceeded the preview caps.
    pub truncated: bool,
    pub server_generation: u64,
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
    pub server_generation: u64,
}

/// Pure helper: build a sha256 hex string from a byte slice.
pub fn sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    format!("{:x}", digest)
}

/// Normalize a `GotoDefinitionResponse`-shaped payload (used by
/// definition, declaration, implementation, typeDefinition) into a
/// uniform `Vec<LocationLink>`. `origin_selection_range` is set to
/// `None` because the upstream response variants do not carry it.
pub fn normalize_goto_response(response: GotoDefinitionResponse) -> Vec<LocationLink> {
    match response {
        GotoDefinitionResponse::Link(links) => links,
        GotoDefinitionResponse::Scalar(loc) => vec![LocationLink {
            origin_selection_range: None,
            target_uri: loc.uri,
            target_range: loc.range,
            target_selection_range: loc.range,
        }],
        GotoDefinitionResponse::Array(locs) => locs
            .into_iter()
            .map(|loc| LocationLink {
                origin_selection_range: None,
                target_uri: loc.uri,
                target_range: loc.range,
                target_selection_range: loc.range,
            })
            .collect(),
    }
}

/// Normalize a `WorkspaceSymbolResponse` (either `Flat(Vec<SymbolInformation>)`
/// or `Nested(Vec<WorkspaceSymbol>)`) into a uniform `Vec<SymbolInformation>`.
///
/// For `WorkspaceSymbol` entries (the 3.17+ shape), the location can be
/// either a full `Location` or a `WorkspaceLocation` (URI only).
/// Workspace-only entries are surfaced with an empty range.
pub fn normalize_workspace_symbol_response(
    response: WorkspaceSymbolResponse,
) -> Vec<SymbolInformation> {
    match response {
        WorkspaceSymbolResponse::Flat(symbols) => symbols,
        WorkspaceSymbolResponse::Nested(symbols) => symbols
            .into_iter()
            .map(|sym| {
                let (uri, range) = match sym.location {
                    lsp_types::OneOf::Left(loc) => (loc.uri, loc.range),
                    lsp_types::OneOf::Right(wl) => {
                        // URI-only — no range available; emit an empty range.
                        (wl.uri, Range::default())
                    }
                };
                SymbolInformation {
                    name: sym.name,
                    kind: sym.kind,
                    tags: sym.tags,
                    location: Location { uri, range },
                    container_name: sym.container_name,
                    #[allow(deprecated)]
                    deprecated: None,
                }
            })
            .collect(),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SourceActionPreviewKind {
    OrganizeImports,
}

impl SourceActionPreviewKind {
    pub fn parse(input: &str) -> Result<Self, LspError> {
        match input {
            "source.organizeImports" | "organizeImports" | "organize_imports" => {
                Ok(Self::OrganizeImports)
            }
            other => Err(LspError::UnsupportedSourceAction(other.to_string())),
        }
    }

    pub fn lsp_kind(self) -> CodeActionKind {
        match self {
            Self::OrganizeImports => CodeActionKind::SOURCE_ORGANIZE_IMPORTS,
        }
    }

    pub fn title(self) -> &'static str {
        match self {
            Self::OrganizeImports => "organize imports",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HierarchyDirection {
    Incoming,
    Outgoing,
    Both,
}

impl HierarchyDirection {
    pub fn parse(input: Option<&str>) -> Result<Self, crate::error::LspError> {
        match input.unwrap_or("both") {
            "incoming" => Ok(Self::Incoming),
            "outgoing" => Ok(Self::Outgoing),
            "both" => Ok(Self::Both),
            other => Err(crate::error::LspError::RequestFailed(format!(
                "unsupported hierarchy direction: {other}"
            ))),
        }
    }
}

/// Pure helper: given a requested action kind and the raw LSP code action
/// responses, select the single best edit-bearing `WorkspaceEdit`.
///
/// Rules:
/// - Raw `Command` variants are rejected.
/// - `CodeAction` with `command: Some(_)` but `edit: None` is command-only
///   (command execution is disabled).
/// - `CodeAction` values without `edit` or `command` are rejected.
/// - Actions whose kind is not hierarchically compatible with the
///   requested kind are rejected.
/// - Exactly one edit-bearing match is returned.
/// - Zero matches → `NoEditForSourceAction` or `CommandOnlySourceAction`.
/// - Multiple matches → `AmbiguousSourceAction`.
pub fn select_source_action_edit(
    requested: SourceActionPreviewKind,
    actions: Vec<CodeActionOrCommand>,
) -> Result<WorkspaceEdit, LspError> {
    let requested_kind = requested.lsp_kind();
    let title = requested.title();

    let mut edit_bearing: Vec<(&CodeAction, &str)> = Vec::new();
    let mut matching_command_only = 0usize;

    for action in &actions {
        match action {
            CodeActionOrCommand::Command(cmd) => {
                trace!(
                    "source action: rejecting raw Command variant: {}",
                    cmd.command
                );
            }
            CodeActionOrCommand::CodeAction(ca) => {
                let kind_matches = match &ca.kind {
                    Some(kind) => {
                        kind == &requested_kind
                            || kind
                                .as_str()
                                .starts_with(&format!("{}.", requested_kind.as_str()))
                    }
                    None => false,
                };
                if !kind_matches {
                    trace!(
                        "source action: rejecting action '{}' with kind {:?}",
                        ca.title,
                        ca.kind
                    );
                    continue;
                }
                if let Some(_edit) = &ca.edit {
                    edit_bearing.push((ca, title));
                } else if ca.command.is_some() {
                    trace!(
                        "source action: rejecting action '{}' (command-only, no edit)",
                        ca.title
                    );
                    matching_command_only += 1;
                } else {
                    trace!(
                        "source action: rejecting action '{}' (no edit, no command)",
                        ca.title
                    );
                }
            }
        }
    }

    match edit_bearing.len() {
        0 => {
            let has_raw_command = actions
                .iter()
                .any(|a| matches!(a, CodeActionOrCommand::Command(_)));
            if has_raw_command || matching_command_only > 0 {
                Err(LspError::CommandOnlySourceAction(title.to_string()))
            } else {
                Err(LspError::NoEditForSourceAction(title.to_string()))
            }
        }
        1 => {
            let (ca, _title) = edit_bearing.remove(0);
            Ok(ca.edit.clone().expect("checked above"))
        }
        _ => {
            let titles: Vec<&str> = edit_bearing
                .iter()
                .map(|(ca, _)| ca.title.as_str())
                .collect();
            Err(LspError::AmbiguousSourceAction(
                title.to_string(),
                titles.join(", "),
            ))
        }
    }
}

/// Default cap on per-completion `detail` strings and `insertText`
/// previews exposed through [`CompletionCandidate`].
pub const COMPLETION_DETAIL_MAX_CHARS: usize = 200;

/// Bounded completion candidate DTO returned to model-facing surfaces.
///
/// Strips raw completion-item edit payloads (`textEdit`,
/// `additionalTextEdits`, `command`) — this surface is read-only and
/// must never apply edits. `detail` and `insert_text_preview` are
/// truncated to [`COMPLETION_DETAIL_MAX_CHARS`] characters each.
///
/// `kind` is the LSP `CompletionItemKind` rendered as a lowercase
/// stable name (`"function"`, `"variable"`, …) for known kinds and as
/// `"kind(N)"` for unknown / custom kinds. Server order is preserved;
/// no client-side sorting is applied.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct CompletionCandidate {
    pub label: String,
    pub detail: Option<String>,
    pub kind: Option<String>,
    pub sort_text: Option<String>,
    pub filter_text: Option<String>,
    pub insert_text_preview: Option<String>,
    pub deprecated: bool,
}

impl CompletionCandidate {
    /// Build a bounded candidate from a raw `CompletionItem`. Pure
    /// conversion — does not touch the network or apply edits.
    pub fn from_completion_item(item: &CompletionItem) -> Self {
        Self {
            label: item.label.clone(),
            detail: item
                .detail
                .as_ref()
                .map(|d| truncate_doc(d, COMPLETION_DETAIL_MAX_CHARS)),
            kind: item.kind.map(completion_kind_to_string),
            sort_text: item.sort_text.clone(),
            filter_text: item.filter_text.clone(),
            insert_text_preview: item
                .insert_text
                .as_ref()
                .map(|t| truncate_doc(t, COMPLETION_DETAIL_MAX_CHARS)),
            deprecated: item.deprecated.unwrap_or(false),
        }
    }
}

/// Render an `CompletionItemKind` as a stable lowercase name. Known
/// LSP kinds map to `"function"`, `"variable"`, …; unknown / custom
/// kinds render as `"kind(<formatted_debug>)"` so the surface stays
/// informative without crashing on server-defined extensions.
pub fn completion_kind_to_string(kind: CompletionItemKind) -> String {
    if kind == CompletionItemKind::TEXT {
        return "text".to_string();
    }
    if kind == CompletionItemKind::METHOD {
        return "method".to_string();
    }
    if kind == CompletionItemKind::FUNCTION {
        return "function".to_string();
    }
    if kind == CompletionItemKind::CONSTRUCTOR {
        return "constructor".to_string();
    }
    if kind == CompletionItemKind::FIELD {
        return "field".to_string();
    }
    if kind == CompletionItemKind::VARIABLE {
        return "variable".to_string();
    }
    if kind == CompletionItemKind::CLASS {
        return "class".to_string();
    }
    if kind == CompletionItemKind::INTERFACE {
        return "interface".to_string();
    }
    if kind == CompletionItemKind::MODULE {
        return "module".to_string();
    }
    if kind == CompletionItemKind::PROPERTY {
        return "property".to_string();
    }
    if kind == CompletionItemKind::UNIT {
        return "unit".to_string();
    }
    if kind == CompletionItemKind::VALUE {
        return "value".to_string();
    }
    if kind == CompletionItemKind::ENUM {
        return "enum".to_string();
    }
    if kind == CompletionItemKind::KEYWORD {
        return "keyword".to_string();
    }
    if kind == CompletionItemKind::SNIPPET {
        return "snippet".to_string();
    }
    if kind == CompletionItemKind::COLOR {
        return "color".to_string();
    }
    if kind == CompletionItemKind::FILE {
        return "file".to_string();
    }
    if kind == CompletionItemKind::REFERENCE {
        return "reference".to_string();
    }
    if kind == CompletionItemKind::FOLDER {
        return "folder".to_string();
    }
    if kind == CompletionItemKind::ENUM_MEMBER {
        return "enum_member".to_string();
    }
    if kind == CompletionItemKind::CONSTANT {
        return "constant".to_string();
    }
    if kind == CompletionItemKind::STRUCT {
        return "struct".to_string();
    }
    if kind == CompletionItemKind::EVENT {
        return "event".to_string();
    }
    if kind == CompletionItemKind::OPERATOR {
        return "operator".to_string();
    }
    if kind == CompletionItemKind::TYPE_PARAMETER {
        return "type_parameter".to_string();
    }
    // Unknown / custom kind — fall back to the Debug representation
    // (which the `lsp_enum!` macro renders as `CompletionItemKind(N)`
    // for unrecognized integer values).
    format!("kind({kind:?})")
}

/// Decoded semantic-token DTO returned to model-facing surfaces.
///
/// `line` and `start` are absolute (not delta-encoded). `start` and
/// `length` are measured in UTF-16 code units, matching the LSP
/// specification. `token_type` is the legend-resolved name; if the
/// server reports an out-of-range index [`decode_semantic_tokens`]
/// returns a structured [`LspError::RequestFailed`] instead of
/// silently dropping the token.
///
/// `modifiers` is a `Vec<String>` of resolved legend names — bit `i`
/// in the wire `token_modifiers_bitset` corresponds to legend
/// position `i`.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct DecodedSemanticToken {
    pub line: u32,
    pub start: u32,
    pub length: u32,
    pub token_type: String,
    pub modifiers: Vec<String>,
}

/// Pure helper: decode an LSP delta-encoded semantic-token stream
/// against a server-supplied legend.
///
/// Decoding rules (per LSP §3.16 semanticTokens):
/// - The first token's `line` is `delta_line` (no previous token).
/// - `line = previous.line + delta_line` for subsequent tokens.
/// - If `delta_line == 0`, the token is on the same line as the
///   previous one and `start = previous.start + delta_start`.
/// - Otherwise, the token is on a new line and `start = delta_start`
///   (absolute on that line).
///
/// Returns [`LspError::RequestFailed`] when a token reports a
/// `token_type` index that exceeds the legend.
pub fn decode_semantic_tokens(
    tokens: &[SemanticToken],
    legend: &SemanticTokenLegendSnapshot,
) -> Result<Vec<DecodedSemanticToken>, LspError> {
    let mut out = Vec::with_capacity(tokens.len());
    let mut prev_line: u32 = 0;
    let mut prev_start: u32 = 0;
    for (i, tok) in tokens.iter().enumerate() {
        let line = prev_line + tok.delta_line;
        let start = if i == 0 || tok.delta_line != 0 {
            tok.delta_start
        } else {
            prev_start + tok.delta_start
        };
        let token_type_idx = tok.token_type as usize;
        let token_type = legend
            .token_types
            .get(token_type_idx)
            .ok_or_else(|| {
                LspError::RequestFailed(format!(
                    "semantic token_type index {token_type_idx} out of range \
                     (legend has {} types)",
                    legend.token_types.len()
                ))
            })?
            .clone();
        let mut modifiers = Vec::new();
        let bitset = tok.token_modifiers_bitset;
        for (bit, name) in legend.token_modifiers.iter().enumerate() {
            if bit >= 32 {
                break;
            }
            if bitset & (1u32 << bit) != 0 {
                modifiers.push(name.clone());
            }
        }
        out.push(DecodedSemanticToken {
            line,
            start,
            length: tok.length,
            token_type,
            modifiers,
        });
        prev_line = line;
        prev_start = start;
    }
    Ok(out)
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

pub struct LspOperations {
    service: std::sync::Arc<LspService>,
}

impl LspOperations {
    pub fn new(service: std::sync::Arc<LspService>) -> Self {
        Self { service }
    }

    /// Look up the [`LspCapabilitySnapshot`] for the client that
    /// services `file_path`. Returns `None` when the client has not
    /// published capabilities yet (i.e. still initializing).
    async fn capability_snapshot_for_file(
        &self,
        file_path: &Path,
    ) -> Option<LspCapabilitySnapshot> {
        let (key, _) = self.service.get_or_create_client(file_path).await.ok()?;
        let caps = self.service.get_capabilities_for_key(&key).await?;
        let lang = detect_language(file_path.to_str().unwrap_or(""));
        let server_name = key.split(':').next_back().map(String::from);
        Some(LspCapabilitySnapshot::from_capabilities(
            &caps,
            server_name.as_deref(),
            lang,
        ))
    }

    /// Fail fast with a structured [`LspUnavailable`] when the server
    /// does not advertise the requested operation.
    ///
    /// Fail-open semantics: if the snapshot is unavailable (client
    /// still initializing, or no caps published yet), the request is
    /// allowed to proceed — the server will respond (likely with
    /// `null` / empty) and the caller can decide what to do.
    async fn require_capability(
        &self,
        file_path: &Path,
        op: LspSemanticOperation,
    ) -> Result<(), LspError> {
        let Some(snapshot) = self.capability_snapshot_for_file(file_path).await else {
            return Ok(());
        };
        if let Some(u) = snapshot.unavailable(op) {
            return Err(LspError::Unavailable(u));
        }
        Ok(())
    }

    pub async fn go_to_definition(
        &self,
        file_path: &Path,
        line: u32,
        column: u32,
    ) -> Result<Vec<LocationLink>, LspError> {
        let (key, _root) = self.service.get_or_create_client(file_path).await?;
        let uri = Url::from_file_path(file_path).map_err(|_| {
            LspError::LaunchFailed(format!("invalid file path: {}", file_path.display()))
        })?;

        let params = serde_json::to_value(GotoDefinitionParams {
            text_document_position_params: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier {
                    uri: url_to_uri(&uri)?,
                },
                position: Position {
                    line,
                    character: column,
                },
            },
            work_done_progress_params: Default::default(),
            partial_result_params: Default::default(),
        })?;

        let resp = self
            .service
            .send_request(&key, "textDocument/definition", params)
            .await?;

        if resp.is_null() {
            return Ok(Vec::new());
        }

        let gdr: GotoDefinitionResponse = serde_json::from_value(resp)?;
        Ok(match gdr {
            GotoDefinitionResponse::Link(links) => links,
            GotoDefinitionResponse::Scalar(loc) => vec![LocationLink {
                origin_selection_range: None,
                target_uri: loc.uri,
                target_range: loc.range,
                target_selection_range: loc.range,
            }],
            GotoDefinitionResponse::Array(locs) => locs
                .into_iter()
                .map(|loc| LocationLink {
                    origin_selection_range: None,
                    target_uri: loc.uri,
                    target_range: loc.range,
                    target_selection_range: loc.range,
                })
                .collect(),
        })
    }

    /// Read-only `textDocument/declaration`. Capability-gated: returns
    /// [`LspError::Unavailable`] with a structured reason when the
    /// server does not advertise `declarationProvider`.
    pub async fn declaration(
        &self,
        file_path: &Path,
        line: u32,
        column: u32,
    ) -> Result<Vec<LocationLink>, LspError> {
        self.require_capability(file_path, LspSemanticOperation::Declaration)
            .await?;
        let (key, _root) = self.service.get_or_create_client(file_path).await?;
        let uri = Url::from_file_path(file_path).map_err(|_| {
            LspError::LaunchFailed(format!("invalid file path: {}", file_path.display()))
        })?;

        let params = serde_json::to_value(GotoDefinitionParams {
            text_document_position_params: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier {
                    uri: url_to_uri(&uri)?,
                },
                position: Position {
                    line,
                    character: column,
                },
            },
            work_done_progress_params: Default::default(),
            partial_result_params: Default::default(),
        })?;

        let resp = self
            .service
            .send_request(&key, "textDocument/declaration", params)
            .await?;

        if resp.is_null() {
            return Ok(Vec::new());
        }

        let gdr: GotoDefinitionResponse = serde_json::from_value(resp)?;
        Ok(normalize_goto_response(gdr))
    }

    /// Read-only `textDocument/implementation`. Capability-gated.
    pub async fn implementation(
        &self,
        file_path: &Path,
        line: u32,
        column: u32,
    ) -> Result<Vec<LocationLink>, LspError> {
        self.require_capability(file_path, LspSemanticOperation::Implementation)
            .await?;
        let (key, _root) = self.service.get_or_create_client(file_path).await?;
        let uri = Url::from_file_path(file_path).map_err(|_| {
            LspError::LaunchFailed(format!("invalid file path: {}", file_path.display()))
        })?;

        // GotoImplementationParams == GotoTypeDefinitionParams == GotoDefinitionParams
        let params = serde_json::to_value(GotoDefinitionParams {
            text_document_position_params: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier {
                    uri: url_to_uri(&uri)?,
                },
                position: Position {
                    line,
                    character: column,
                },
            },
            work_done_progress_params: Default::default(),
            partial_result_params: Default::default(),
        })?;

        let resp = self
            .service
            .send_request(&key, "textDocument/implementation", params)
            .await?;

        if resp.is_null() {
            return Ok(Vec::new());
        }

        let gdr: GotoDefinitionResponse = serde_json::from_value(resp)?;
        Ok(normalize_goto_response(gdr))
    }

    /// Read-only `textDocument/documentHighlight`. Capability-gated.
    /// Returns the raw `DocumentHighlight` entries (preserving the
    /// optional `kind` of Text / Read / Write).
    pub async fn document_highlights(
        &self,
        file_path: &Path,
        line: u32,
        column: u32,
    ) -> Result<Vec<DocumentHighlight>, LspError> {
        self.require_capability(file_path, LspSemanticOperation::DocumentHighlight)
            .await?;
        let (key, _root) = self.service.get_or_create_client(file_path).await?;
        let uri = Url::from_file_path(file_path).map_err(|_| {
            LspError::LaunchFailed(format!("invalid file path: {}", file_path.display()))
        })?;

        let params = serde_json::to_value(DocumentHighlightParams {
            text_document_position_params: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier {
                    uri: url_to_uri(&uri)?,
                },
                position: Position {
                    line,
                    character: column,
                },
            },
            work_done_progress_params: Default::default(),
            partial_result_params: Default::default(),
        })?;

        let resp = self
            .service
            .send_request(&key, "textDocument/documentHighlight", params)
            .await?;

        if resp.is_null() {
            return Ok(Vec::new());
        }

        let highlights: Vec<DocumentHighlight> = serde_json::from_value(resp)?;
        Ok(highlights)
    }

    /// Read-only `workspace/symbol` query. Capability-gated.
    ///
    /// Servers may respond with the flat (`Vec<SymbolInformation>`) or
    /// nested (`Vec<WorkspaceSymbol>`) shape depending on the
    /// negotiated capabilities. Both are normalized to
    /// `Vec<SymbolInformation>`.
    pub async fn workspace_symbols(&self, query: &str) -> Result<Vec<SymbolInformation>, LspError> {
        // workspace/symbol has no file path; pick a key from the
        // service-level client inventory. If there are no clients we
        // still allow the request to proceed (the service will route
        // it through `send_request` which uses the JSON-RPC routing
        // infrastructure).
        let key = self.first_client_key().await.ok_or_else(|| {
            LspError::NotInitialized("no LSP client available for workspace_symbols".to_string())
        })?;

        if let Some(snapshot) = self.capability_snapshot_for_any_key(&key).await {
            if let Some(u) = snapshot.unavailable(LspSemanticOperation::WorkspaceSymbols) {
                return Err(LspError::Unavailable(u));
            }
        }

        let params = serde_json::to_value(WorkspaceSymbolParams {
            query: query.to_string(),
            partial_result_params: Default::default(),
            work_done_progress_params: Default::default(),
        })?;

        let resp = self
            .service
            .send_request(&key, "workspace/symbol", params)
            .await?;

        if resp.is_null() {
            return Ok(Vec::new());
        }

        let response: WorkspaceSymbolResponse = serde_json::from_value(resp)?;
        Ok(normalize_workspace_symbol_response(response))
    }

    /// Look up the [`LspCapabilitySnapshot`] for an arbitrary client
    /// key (used by workspace-scoped operations where no file path is
    /// available).
    async fn capability_snapshot_for_any_key(&self, key: &str) -> Option<LspCapabilitySnapshot> {
        let caps = self.service.get_capabilities_for_key(key).await?;
        let server_name = key.split(':').next_back().map(String::from);
        Some(LspCapabilitySnapshot::from_capabilities(
            &caps,
            server_name.as_deref(),
            None,
        ))
    }

    /// Return the first known client key, used as the routing target
    /// for workspace-scoped requests.
    async fn first_client_key(&self) -> Option<String> {
        self.service.client_keys().await.into_iter().next()
    }

    pub async fn find_references(
        &self,
        file_path: &Path,
        line: u32,
        column: u32,
    ) -> Result<Vec<Location>, LspError> {
        let (key, _root) = self.service.get_or_create_client(file_path).await?;
        let uri = Url::from_file_path(file_path).map_err(|_| {
            LspError::LaunchFailed(format!("invalid file path: {}", file_path.display()))
        })?;

        let params = serde_json::to_value(ReferenceParams {
            text_document_position: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier {
                    uri: url_to_uri(&uri)?,
                },
                position: Position {
                    line,
                    character: column,
                },
            },
            context: ReferenceContext {
                include_declaration: true,
            },
            work_done_progress_params: Default::default(),
            partial_result_params: Default::default(),
        })?;

        let resp = self
            .service
            .send_request(&key, "textDocument/references", params)
            .await?;

        if resp.is_null() {
            return Ok(Vec::new());
        }

        let refs: Vec<Location> = serde_json::from_value(resp)?;
        Ok(refs)
    }

    pub async fn hover(
        &self,
        file_path: &Path,
        line: u32,
        column: u32,
    ) -> Result<Option<String>, LspError> {
        let (key, _root) = self.service.get_or_create_client(file_path).await?;
        let uri = Url::from_file_path(file_path).map_err(|_| {
            LspError::LaunchFailed(format!("invalid file path: {}", file_path.display()))
        })?;

        let params = serde_json::to_value(HoverParams {
            text_document_position_params: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier {
                    uri: url_to_uri(&uri)?,
                },
                position: Position {
                    line,
                    character: column,
                },
            },
            work_done_progress_params: Default::default(),
        })?;

        let resp = self
            .service
            .send_request(&key, "textDocument/hover", params)
            .await?;

        if resp.is_null() {
            return Ok(None);
        }

        let hover: Hover = match serde_json::from_value(resp) {
            Ok(h) => h,
            Err(e) => {
                trace!("failed to parse hover response: {}", e);
                return Ok(None);
            }
        };
        Ok(Some(format_hover_contents(&hover.contents)))
    }

    pub async fn document_symbols(
        &self,
        file_path: &Path,
    ) -> Result<Vec<DocumentSymbol>, LspError> {
        let (key, _root) = self.service.get_or_create_client(file_path).await?;
        let uri = Url::from_file_path(file_path).map_err(|_| {
            LspError::LaunchFailed(format!("invalid file path: {}", file_path.display()))
        })?;

        let params = serde_json::to_value(DocumentSymbolParams {
            text_document: TextDocumentIdentifier {
                uri: url_to_uri(&uri)?,
            },
            work_done_progress_params: Default::default(),
            partial_result_params: Default::default(),
        })?;

        let resp = self
            .service
            .send_request(&key, "textDocument/documentSymbol", params)
            .await?;

        if resp.is_null() {
            return Ok(Vec::new());
        }

        let symbols: Vec<DocumentSymbol> = serde_json::from_value(resp)?;
        Ok(symbols)
    }

    pub async fn code_actions(
        &self,
        file_path: &Path,
        start_line: u32,
        start_col: u32,
        end_line: u32,
        end_col: u32,
        diagnostics: Vec<Diagnostic>,
        only: Option<Vec<CodeActionKind>>,
    ) -> Result<Vec<CodeActionOrCommand>, LspError> {
        let (key, _root) = self.service.get_or_create_client(file_path).await?;
        let uri = Url::from_file_path(file_path).map_err(|_| {
            LspError::LaunchFailed(format!("invalid file path: {}", file_path.display()))
        })?;

        let range = Range {
            start: Position {
                line: start_line,
                character: start_col,
            },
            end: Position {
                line: end_line,
                character: end_col,
            },
        };

        let params = serde_json::to_value(CodeActionParams {
            text_document: TextDocumentIdentifier {
                uri: url_to_uri(&uri)?,
            },
            range,
            context: CodeActionContext {
                diagnostics,
                only,
                trigger_kind: None,
            },
            work_done_progress_params: Default::default(),
            partial_result_params: Default::default(),
        })?;

        let resp = self
            .service
            .send_request(&key, "textDocument/codeAction", params)
            .await?;

        if resp.is_null() {
            return Ok(Vec::new());
        }

        let actions: Vec<CodeActionOrCommand> = serde_json::from_value(resp)?;
        Ok(actions)
    }

    pub async fn completion(
        &self,
        file_path: &Path,
        line: u32,
        column: u32,
        trigger_kind: Option<CompletionTriggerKind>,
        trigger_char: Option<String>,
    ) -> Result<Vec<CompletionItem>, LspError> {
        let (key, _root) = self.service.get_or_create_client(file_path).await?;
        let uri = Url::from_file_path(file_path).map_err(|_| {
            LspError::LaunchFailed(format!("invalid file path: {}", file_path.display()))
        })?;

        let params = serde_json::to_value(CompletionParams {
            text_document_position: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier {
                    uri: url_to_uri(&uri)?,
                },
                position: Position {
                    line,
                    character: column,
                },
            },
            work_done_progress_params: Default::default(),
            partial_result_params: Default::default(),
            context: trigger_kind.map(|kind| CompletionContext {
                trigger_kind: kind,
                trigger_character: trigger_char,
            }),
        })?;

        let resp = self
            .service
            .send_request(&key, "textDocument/completion", params)
            .await?;

        if resp.is_null() {
            return Ok(Vec::new());
        }

        let items: Vec<CompletionItem> =
            match serde_json::from_value::<CompletionList>(resp.clone()) {
                Ok(list) => list.items,
                Err(_) => serde_json::from_value(resp).unwrap_or_default(),
            };
        Ok(items)
    }

    /// Bounded, read-only `textDocument/completion` returning typed
    /// [`CompletionCandidate`] DTOs. Capability-gated: returns
    /// [`LspError::Unavailable`] when the server does not advertise a
    /// completion provider.
    ///
    /// The output is truncated to at most `max_candidates` items,
    /// preserving server order (no client-side sort). Raw completion
    /// edit payloads (`textEdit`, `additionalTextEdits`, `command`)
    /// are stripped — this surface is read-only and must never apply
    /// edits. `detail` and `insert_text_preview` are each capped at
    /// [`COMPLETION_DETAIL_MAX_CHARS`] characters.
    pub async fn completion_bounded(
        &self,
        file_path: &Path,
        line: u32,
        column: u32,
        trigger_kind: Option<CompletionTriggerKind>,
        trigger_char: Option<String>,
        max_candidates: usize,
    ) -> Result<Vec<CompletionCandidate>, LspError> {
        self.require_capability(file_path, LspSemanticOperation::Completion)
            .await?;
        let items = self
            .completion(file_path, line, column, trigger_kind, trigger_char)
            .await?;
        Ok(items
            .iter()
            .take(max_candidates)
            .map(CompletionCandidate::from_completion_item)
            .collect())
    }

    /// Read-only `textDocument/semanticTokens/full` returning typed
    /// [`DecodedSemanticToken`] DTOs with legend-resolved type and
    /// modifier names. Capability-gated: returns
    /// [`LspError::Unavailable`] when the server does not advertise a
    /// semantic-tokens provider.
    ///
    /// The output is truncated to at most `max_tokens` decoded
    /// tokens. Token type / modifier indexes that exceed the legend
    /// are reported as [`LspError::RequestFailed`]; this is the
    /// structured fallback for misbehaving servers rather than a
    /// silent drop.
    pub async fn semantic_tokens(
        &self,
        file_path: &Path,
        max_tokens: usize,
    ) -> Result<Vec<DecodedSemanticToken>, LspError> {
        self.require_capability(file_path, LspSemanticOperation::SemanticTokens)
            .await?;
        let (key, _root) = self.service.get_or_create_client(file_path).await?;
        let uri = Url::from_file_path(file_path).map_err(|_| {
            LspError::LaunchFailed(format!("invalid file path: {}", file_path.display()))
        })?;

        let params = serde_json::to_value(SemanticTokensParams {
            text_document: TextDocumentIdentifier {
                uri: url_to_uri(&uri)?,
            },
            work_done_progress_params: Default::default(),
            partial_result_params: Default::default(),
        })?;

        let resp = self
            .service
            .send_request(&key, "textDocument/semanticTokens/full", params)
            .await?;

        if resp.is_null() {
            return Ok(Vec::new());
        }

        let tokens: SemanticTokens = match serde_json::from_value(resp) {
            Ok(t) => t,
            Err(e) => {
                trace!("failed to parse semanticTokens response: {}", e);
                return Err(LspError::RequestFailed(format!(
                    "malformed semanticTokens response: {e}"
                )));
            }
        };

        if tokens.data.is_empty() {
            return Ok(Vec::new());
        }

        let legend = self
            .capability_snapshot_for_file(file_path)
            .await
            .and_then(|snap| snap.details.semantic_token_legend)
            .ok_or_else(|| {
                LspError::RequestFailed(
                    "semantic token legend unavailable for this client".to_string(),
                )
            })?;

        let decoded = decode_semantic_tokens(&tokens.data, &legend)?;
        Ok(decoded.into_iter().take(max_tokens).collect())
    }

    /// Read-only `textDocument/signatureHelp` returning a typed
    /// [`SignatureHelpSummary`] DTO. Capability-gated.
    ///
    /// Documentation strings are truncated to
    /// [`SIGNATURE_DOC_MAX_CHARS`] per item. Parameter labels expressed
    /// as `[start, end]` offsets are resolved to substrings of the
    /// signature label. Returns `None` when the server responds with
    /// `null` or with no signatures.
    pub async fn signature_help_typed(
        &self,
        file_path: &Path,
        line: u32,
        column: u32,
    ) -> Result<Option<SignatureHelpSummary>, LspError> {
        self.require_capability(file_path, LspSemanticOperation::SignatureHelp)
            .await?;
        let (key, _root) = self.service.get_or_create_client(file_path).await?;
        let uri = Url::from_file_path(file_path).map_err(|_| {
            LspError::LaunchFailed(format!("invalid file path: {}", file_path.display()))
        })?;

        let params = serde_json::to_value(SignatureHelpParams {
            text_document_position_params: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier {
                    uri: url_to_uri(&uri)?,
                },
                position: Position {
                    line,
                    character: column,
                },
            },
            work_done_progress_params: Default::default(),
            context: None,
        })?;

        let resp = self
            .service
            .send_request(&key, "textDocument/signatureHelp", params)
            .await?;

        if resp.is_null() {
            return Ok(None);
        }

        let help: SignatureHelp = match serde_json::from_value(resp) {
            Ok(h) => h,
            Err(e) => {
                trace!("failed to parse signature help response: {}", e);
                return Ok(None);
            }
        };
        Ok(SignatureHelpSummary::from_signature_help(&help))
    }

    /// Backwards-compatible string rendering of signature help.
    ///
    /// Delegates to [`Self::signature_help_typed`] and renders the
    /// normalized summary to a plain-text representation. Returns
    /// `None` when the server has no signature help at this position.
    pub async fn signature_help(
        &self,
        file_path: &Path,
        line: u32,
        column: u32,
    ) -> Result<Option<String>, LspError> {
        match self.signature_help_typed(file_path, line, column).await? {
            Some(summary) => Ok(Some(format_signature_help_typed(&summary))),
            None => Ok(None),
        }
    }

    pub async fn code_lens(&self, file_path: &Path) -> Result<Vec<CodeLens>, LspError> {
        let (key, _root) = self.service.get_or_create_client(file_path).await?;
        let uri = Url::from_file_path(file_path).map_err(|_| {
            LspError::LaunchFailed(format!("invalid file path: {}", file_path.display()))
        })?;

        let params = serde_json::to_value(CodeLensParams {
            text_document: TextDocumentIdentifier {
                uri: url_to_uri(&uri)?,
            },
            work_done_progress_params: Default::default(),
            partial_result_params: Default::default(),
        })?;

        let resp = self
            .service
            .send_request(&key, "textDocument/codeLens", params)
            .await?;

        if resp.is_null() {
            return Ok(Vec::new());
        }

        let lenses: Vec<CodeLens> = serde_json::from_value(resp)?;
        Ok(lenses)
    }

    pub async fn prepare_rename(
        &self,
        file_path: &Path,
        line: u32,
        column: u32,
    ) -> Result<Option<PrepareRenameResponse>, LspError> {
        let (key, _root) = self.service.get_or_create_client(file_path).await?;
        let uri = Url::from_file_path(file_path).map_err(|_| {
            LspError::LaunchFailed(format!("invalid file path: {}", file_path.display()))
        })?;

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

    pub async fn rename_preview(
        &self,
        file_path: &Path,
        line: u32,
        column: u32,
        new_name: &str,
        allowed_root: Option<&Path>,
    ) -> Result<WorkspaceEditPreview, LspError> {
        let (key, _uri_str) = self.service.ensure_file_open_from_disk(file_path).await?;
        let uri = Url::from_file_path(file_path).map_err(|_| {
            LspError::LaunchFailed(format!("invalid file path: {}", file_path.display()))
        })?;

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

    pub async fn format_preview(
        &self,
        file_path: &Path,
        allowed_root: Option<&Path>,
    ) -> Result<WorkspaceEditPreview, LspError> {
        let (key, _uri_str) = self.service.ensure_file_open_from_disk(file_path).await?;
        let uri = Url::from_file_path(file_path).map_err(|_| {
            LspError::LaunchFailed(format!("invalid file path: {}", file_path.display()))
        })?;

        let params = serde_json::to_value(DocumentFormattingParams {
            text_document: TextDocumentIdentifier {
                uri: url_to_uri(&uri)?,
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
        // Check capability up-front. Fail-open if the snapshot is
        // unavailable (server still initializing).
        if let Some(snapshot) = self.capability_snapshot_for_file(file_path).await {
            if !snapshot.supports(LspSemanticOperation::PrepareRename) {
                if let Some(u) = snapshot.unavailable(LspSemanticOperation::PrepareRename) {
                    return Ok(PrepareRenameResult::Unavailable(u));
                }
            }
        }
        let resp = self.prepare_rename(file_path, line, column).await?;
        Ok(PrepareRenameResult::from_response(resp))
    }

    /// Preview-only `textDocument/rename` returning a typed
    /// [`RenamePreview`] DTO. Capability-gated via
    /// `prepare_rename_typed` and the same root-validation
    /// contract as [`Self::rename_preview`].
    ///
    /// `new_name` must be non-empty. The on-disk file is never
    /// mutated. Resource operations (create/rename/delete) inside
    /// `document_changes` are reported as structured warnings
    /// because the underlying preview pipeline does not surface
    /// them.
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

        // Step 1: prepare_rename (typed) → placeholder.
        let prepared = self.prepare_rename_typed(file_path, line, column).await?;
        let old_name = match &prepared {
            PrepareRenameResult::Range { placeholder, .. } => placeholder.clone(),
            PrepareRenameResult::DefaultBehavior { .. } | PrepareRenameResult::Unavailable(_) => {
                None
            }
        };

        // Step 2: call the existing rename pipeline to get a
        // raw WorkspaceEdit (so we can inspect document_changes
        // for resource ops) AND the prepared WorkspaceEditPreview.
        let (raw_edit, preview) = self
            .rename_raw_and_preview(file_path, line, column, new_name, allowed_root)
            .await?;

        // Step 3: scan for resource operations in document_changes.
        let mut warnings: Vec<String> = Vec::new();
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

        // Step 4: re-check the caps from the prepared preview.
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

        let (key, _root) = self.service.get_or_create_client(file_path).await?;
        let server_generation = self.service.generation_for_key(&key).await;

        Ok(RenamePreview {
            old_name,
            new_name: new_name.to_string(),
            affected_files: preview.files,
            edit_count,
            warnings,
            truncated,
            server_generation,
        })
    }

    /// Private helper: run the rename pipeline and return BOTH
    /// the raw `WorkspaceEdit` (for resource-op inspection) AND
    /// the prepared `WorkspaceEditPreview` (for the model-facing
    /// surface). Reuses the same logic as the public
    /// `rename_preview` method.
    async fn rename_raw_and_preview(
        &self,
        file_path: &Path,
        line: u32,
        column: u32,
        new_name: &str,
        allowed_root: Option<&Path>,
    ) -> Result<(WorkspaceEdit, WorkspaceEditPreview), LspError> {
        let (key, _uri_str) = self.service.ensure_file_open_from_disk(file_path).await?;
        let uri = Url::from_file_path(file_path).map_err(|_| {
            LspError::LaunchFailed(format!("invalid file path: {}", file_path.display()))
        })?;

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

    // ── Phase 4 Pass 7: typed code-action surface ───────────────────

    /// Bounded, read-only `textDocument/codeAction` summary DTOs.
    /// Capability-gated: returns `LspUnavailable` when the server
    /// does not advertise a code-action provider.
    ///
    /// The output is truncated to at most `max_actions` items
    /// preserving server order. Raw `Command` payloads are
    /// surfaced as summaries with `has_command = true` and
    /// `has_edit = false`; the surface never executes commands.
    pub async fn code_action_summaries(
        &self,
        file_path: &Path,
        start_line: u32,
        start_col: u32,
        end_line: u32,
        end_col: u32,
        diagnostics: Vec<Diagnostic>,
        only: Option<Vec<CodeActionKind>>,
        max_actions: usize,
    ) -> Result<Vec<CodeActionSummary>, LspError> {
        if max_actions == 0 {
            return Ok(Vec::new());
        }
        if let Some(snapshot) = self.capability_snapshot_for_file(file_path).await {
            if !snapshot.supports(LspSemanticOperation::CodeAction) {
                if let Some(u) = snapshot.unavailable(LspSemanticOperation::CodeAction) {
                    return Err(LspError::Unavailable(u));
                }
            }
        }

        let actions = self
            .code_actions(
                file_path,
                start_line,
                start_col,
                end_line,
                end_col,
                diagnostics,
                only,
            )
            .await?;

        Ok(actions
            .iter()
            .take(max_actions)
            .map(CodeActionSummary::from_action)
            .collect())
    }

    /// Preview a single code action identified by `action_index`
    /// (the index into the same ordering produced by
    /// `code_action_summaries`). Capability-gated via
    /// `code_action_summaries`.
    ///
    /// Returns [`LspError::CommandOnlyCodeAction`] when the
    /// resolved action is a raw `Command` (the surface never
    /// executes commands) or when the resolved `CodeAction` has
    /// `command: Some(_)` but no `edit` payload. The on-disk
    /// file is never mutated.
    pub async fn preview_code_action(
        &self,
        file_path: &Path,
        start_line: u32,
        start_col: u32,
        end_line: u32,
        end_col: u32,
        diagnostics: Vec<Diagnostic>,
        only: Option<Vec<CodeActionKind>>,
        action_index: usize,
        allowed_root: Option<&Path>,
    ) -> Result<CodeActionPreview, LspError> {
        // Capability-gate by running the same check as summaries
        // (without consuming a max_actions budget).
        if let Some(snapshot) = self.capability_snapshot_for_file(file_path).await {
            if !snapshot.supports(LspSemanticOperation::CodeAction) {
                if let Some(u) = snapshot.unavailable(LspSemanticOperation::CodeAction) {
                    return Err(LspError::Unavailable(u));
                }
            }
        }

        let actions = self
            .code_actions(
                file_path,
                start_line,
                start_col,
                end_line,
                end_col,
                diagnostics,
                only,
            )
            .await?;

        let action = actions.get(action_index).ok_or_else(|| {
            LspError::RequestFailed(format!(
                "action_index {} out of range (server returned {} actions)",
                action_index,
                actions.len()
            ))
        })?;

        let (title, kind, ws_edit) = match action {
            CodeActionOrCommand::Command(cmd) => {
                return Err(LspError::CommandOnlyCodeAction(cmd.title.clone()));
            }
            CodeActionOrCommand::CodeAction(ca) => {
                if ca.edit.is_none() {
                    // Command-only (or empty) CodeAction. The
                    // surface never executes commands; reject
                    // with the same error type.
                    return Err(LspError::CommandOnlyCodeAction(ca.title.clone()));
                }
                (
                    ca.title.clone(),
                    ca.kind.as_ref().map(|k| k.as_str().to_string()),
                    ca.edit.clone().expect("edit checked above"),
                )
            }
        };

        let preview = preview_workspace_edit("code action", ws_edit, allowed_root)?;

        // preview_workspace_edit already errors on resource ops
        // via UnsupportedEdit; we only surface edits-only shapes
        // here, so the warning bucket stays empty.
        let warnings: Vec<String> = Vec::new();

        let (key, _root) = self.service.get_or_create_client(file_path).await?;
        let server_generation = self.service.generation_for_key(&key).await;

        Ok(CodeActionPreview {
            title,
            kind,
            affected_files: preview.files,
            edit_count: preview.total_edits,
            warnings,
            truncated: preview.truncated,
            server_generation,
        })
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
        let after_disk_hash = sha256_hex(&tokio::fs::read(file_path).await.map_err(|e| {
            LspError::RequestFailed(format!(
                "failed to re-read file {}: {}",
                file_path.display(),
                e
            ))
        })?);
        if after_disk_hash != before_hash {
            return Err(LspError::RequestFailed(format!(
                "format_preview_typed: on-disk file {} changed unexpectedly \
                 (before_hash={}, after_disk_hash={})",
                file_path.display(),
                before_hash,
                after_disk_hash
            )));
        }

        let (key, _root) = self.service.get_or_create_client(file_path).await?;
        let server_generation = self.service.generation_for_key(&key).await;

        Ok(FormattingPreview {
            file: file_path.to_path_buf(),
            edit_count: preview.total_edits,
            before_hash,
            after_hash,
            diff,
            truncated,
            server_generation,
        })
    }

    pub async fn source_action_preview(
        &self,
        file_path: &Path,
        action: SourceActionPreviewKind,
        allowed_root: Option<&Path>,
    ) -> Result<WorkspaceEditPreview, LspError> {
        let (key, _uri_str) = self.service.ensure_file_open_from_disk(file_path).await?;
        let uri = Url::from_file_path(file_path).map_err(|_| {
            LspError::LaunchFailed(format!("invalid file path: {}", file_path.display()))
        })?;

        let text = tokio::fs::read_to_string(file_path).await.map_err(|e| {
            LspError::RequestFailed(format!(
                "failed to read file {}: {}",
                file_path.display(),
                e
            ))
        })?;
        let end = document_end_position_utf16(&text);

        let params = serde_json::to_value(CodeActionParams {
            text_document: TextDocumentIdentifier {
                uri: url_to_uri(&uri)?,
            },
            range: Range {
                start: Position {
                    line: 0,
                    character: 0,
                },
                end,
            },
            context: CodeActionContext {
                diagnostics: vec![],
                only: Some(vec![action.lsp_kind()]),
                trigger_kind: Some(CodeActionTriggerKind::INVOKED),
            },
            work_done_progress_params: Default::default(),
            partial_result_params: Default::default(),
        })?;

        let resp = self
            .service
            .send_request(&key, "textDocument/codeAction", params)
            .await?;

        if resp.is_null() {
            return Err(LspError::NoEditForSourceAction(action.title().to_string()));
        }

        let actions: Vec<CodeActionOrCommand> = serde_json::from_value(resp)?;
        let ws_edit = crate::operations::select_source_action_edit(action, actions)?;
        preview_workspace_edit(action.title(), ws_edit, allowed_root)
    }

    pub async fn semantic_check_preview(
        &self,
        file_path: &Path,
        proposed_text: String,
        allowed_root: Option<&Path>,
    ) -> Result<SemanticCheckPreview, LspError> {
        const OVERLAY_DIAGNOSTIC_WAIT_MS: u64 = 250;
        const MAX_OVERLAY_DIAGNOSTICS: usize = 100;
        const MAX_OVERLAY_SYMBOLS: usize = 200;

        validate_path_against_root(file_path, allowed_root)?;

        let overlay = OverlaySession::new(self.service.clone());
        let token = overlay.apply_overlay(file_path, &proposed_text).await?;

        tokio::time::sleep(tokio::time::Duration::from_millis(
            OVERLAY_DIAGNOSTIC_WAIT_MS,
        ))
        .await;

        let warming = self
            .service
            .diagnostics_may_still_be_warming(&token.key, &token.uri)
            .await;

        let diag_result = self
            .service
            .get_diagnostics_for_key(&token.key, &token.uri)
            .await;

        let sym_result = self.document_symbols(file_path).await;

        let restore_result = overlay.restore(&token).await;

        let (diagnostics, diagnostics_error) = match diag_result {
            Ok(raw) => (
                raw.into_iter()
                    .take(MAX_OVERLAY_DIAGNOSTICS)
                    .map(|d| diagnostic_to_file_diagnostic(&token.uri, d))
                    .collect(),
                None,
            ),
            Err(e) => (Vec::new(), Some(e.to_string())),
        };

        let (symbols, symbols_error) = match sym_result {
            Ok(raw) => {
                let mut symbols = Vec::new();
                let mut remaining = MAX_OVERLAY_SYMBOLS;
                flatten_symbols(&raw, &mut symbols, &mut remaining);
                (symbols, None)
            }
            Err(e) => (Vec::new(), Some(e.to_string())),
        };

        let (restored_disk_view, restore_error) = match restore_result {
            Ok(()) => (true, None),
            Err(e) => {
                tracing::warn!(file = %file_path.display(), error = %e, "overlay restore failed");
                (false, Some(e.to_string()))
            }
        };

        Ok(SemanticCheckPreview {
            file: token.uri,
            diagnostics_may_still_be_warming: warming,
            diagnostics,
            diagnostics_error,
            symbols,
            symbols_error,
            restored_disk_view,
            restore_error,
        })
    }

    pub async fn prepare_call_hierarchy(
        &self,
        file_path: &Path,
        line: u32,
        column: u32,
    ) -> Result<Vec<CallHierarchyItem>, LspError> {
        let (key, _root) = self.service.ensure_file_open_from_disk(file_path).await?;
        let uri = Url::from_file_path(file_path).map_err(|_| {
            LspError::LaunchFailed(format!("invalid file path: {}", file_path.display()))
        })?;

        let params = serde_json::to_value(CallHierarchyPrepareParams {
            text_document_position_params: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier {
                    uri: url_to_uri(&uri)?,
                },
                position: Position {
                    line,
                    character: column,
                },
            },
            work_done_progress_params: Default::default(),
        })?;

        let resp = self
            .service
            .send_request(&key, "textDocument/prepareCallHierarchy", params)
            .await?;

        if resp.is_null() {
            return Ok(Vec::new());
        }

        let items: Vec<CallHierarchyItem> = serde_json::from_value(resp)?;
        Ok(items)
    }

    pub async fn incoming_calls(
        &self,
        item: CallHierarchyItem,
    ) -> Result<Vec<CallHierarchyIncomingCall>, LspError> {
        let file_path = uri_to_file_path(&item.uri)?;
        let (key, _root) = self.service.get_or_create_client(&file_path).await?;

        let params = serde_json::to_value(CallHierarchyIncomingCallsParams {
            item,
            work_done_progress_params: Default::default(),
            partial_result_params: Default::default(),
        })?;

        let resp = self
            .service
            .send_request(&key, "callHierarchy/incomingCalls", params)
            .await?;

        if resp.is_null() {
            return Ok(Vec::new());
        }

        let calls: Vec<CallHierarchyIncomingCall> = serde_json::from_value(resp)?;
        Ok(calls)
    }

    pub async fn outgoing_calls(
        &self,
        item: CallHierarchyItem,
    ) -> Result<Vec<CallHierarchyOutgoingCall>, LspError> {
        let file_path = uri_to_file_path(&item.uri)?;
        let (key, _root) = self.service.get_or_create_client(&file_path).await?;

        let params = serde_json::to_value(CallHierarchyOutgoingCallsParams {
            item,
            work_done_progress_params: Default::default(),
            partial_result_params: Default::default(),
        })?;

        let resp = self
            .service
            .send_request(&key, "callHierarchy/outgoingCalls", params)
            .await?;

        if resp.is_null() {
            return Ok(Vec::new());
        }

        let calls: Vec<CallHierarchyOutgoingCall> = serde_json::from_value(resp)?;
        Ok(calls)
    }

    pub async fn prepare_type_hierarchy(
        &self,
        file_path: &Path,
        line: u32,
        column: u32,
    ) -> Result<Vec<TypeHierarchyItem>, LspError> {
        let (key, _root) = self.service.ensure_file_open_from_disk(file_path).await?;
        let uri = Url::from_file_path(file_path).map_err(|_| {
            LspError::LaunchFailed(format!("invalid file path: {}", file_path.display()))
        })?;

        let params = serde_json::to_value(TypeHierarchyPrepareParams {
            text_document_position_params: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier {
                    uri: url_to_uri(&uri)?,
                },
                position: Position {
                    line,
                    character: column,
                },
            },
            work_done_progress_params: Default::default(),
        })?;

        let resp = self
            .service
            .send_request(&key, "textDocument/prepareTypeHierarchy", params)
            .await?;

        if resp.is_null() {
            return Ok(Vec::new());
        }

        let items: Vec<TypeHierarchyItem> = serde_json::from_value(resp)?;
        Ok(items)
    }

    pub async fn supertypes(
        &self,
        item: TypeHierarchyItem,
    ) -> Result<Vec<TypeHierarchyItem>, LspError> {
        let file_path = uri_to_file_path(&item.uri)?;
        let (key, _root) = self.service.get_or_create_client(&file_path).await?;

        let params = serde_json::to_value(TypeHierarchySupertypesParams {
            item,
            work_done_progress_params: Default::default(),
            partial_result_params: Default::default(),
        })?;

        let resp = self
            .service
            .send_request(&key, "typeHierarchy/supertypes", params)
            .await?;

        if resp.is_null() {
            return Ok(Vec::new());
        }

        let items: Vec<TypeHierarchyItem> = serde_json::from_value(resp)?;
        Ok(items)
    }

    pub async fn subtypes(
        &self,
        item: TypeHierarchyItem,
    ) -> Result<Vec<TypeHierarchyItem>, LspError> {
        let file_path = uri_to_file_path(&item.uri)?;
        let (key, _root) = self.service.get_or_create_client(&file_path).await?;

        let params = serde_json::to_value(TypeHierarchySubtypesParams {
            item,
            work_done_progress_params: Default::default(),
            partial_result_params: Default::default(),
        })?;

        let resp = self
            .service
            .send_request(&key, "typeHierarchy/subtypes", params)
            .await?;

        if resp.is_null() {
            return Ok(Vec::new());
        }

        let items: Vec<TypeHierarchyItem> = serde_json::from_value(resp)?;
        Ok(items)
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
    // Walk the edits in reverse and apply each one in-place using
    // the line/character offsets directly (line/col are 0-based;
    // chars are UTF-16 code units per LSP semantics — for the
    // simple ASCII cases a format pass operates on, this is
    // accurate).
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
fn build_bounded_unified_diff(before: &str, after: &str, file_path: &Path) -> (String, bool) {
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

fn uri_to_file_path(uri: &Uri) -> Result<std::path::PathBuf, LspError> {
    let url = Url::parse(uri.as_str())
        .map_err(|e| LspError::RequestFailed(format!("invalid LSP URI: {e}")))?;
    url.to_file_path()
        .map_err(|_| LspError::RequestFailed(format!("URI is not a file path: {}", uri.as_str())))
}

fn format_hover_contents(contents: &HoverContents) -> String {
    match contents {
        HoverContents::Scalar(s) => match s {
            MarkedString::String(s) => s.clone(),
            MarkedString::LanguageString(ls) => {
                format!("```{}\n{}\n```", ls.language, ls.value)
            }
        },
        HoverContents::Array(arr) => arr
            .iter()
            .map(|s| match s {
                MarkedString::String(s) => s.clone(),
                MarkedString::LanguageString(ls) => {
                    format!("```{}\n{}\n```", ls.language, ls.value)
                }
            })
            .collect::<Vec<_>>()
            .join("\n\n"),
        HoverContents::Markup(mc) => mc.value.clone(),
    }
}

fn format_documentation(doc: &Documentation) -> String {
    match doc {
        Documentation::String(s) => s.clone(),
        Documentation::MarkupContent(mc) => mc.value.clone(),
    }
}

fn format_signature_help_typed(help: &SignatureHelpSummary) -> String {
    let mut result = String::new();
    for (i, sig) in help.signatures.iter().enumerate() {
        if i > 0 {
            result.push_str("\n---\n");
        }
        result.push_str(&sig.label);
        if let Some(doc) = &sig.documentation {
            result.push_str("\n\n");
            result.push_str(doc);
        }
        for (j, param) in sig.parameters.iter().enumerate() {
            let doc_str = param.documentation.as_deref().unwrap_or("");
            result.push_str(&format!("\n  {}. {}: {}", j + 1, param.label, doc_str));
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::capability::LspUnavailable;
    use crate::TextEditPreview;
    use lsp_types::{MarkupContent, MarkupKind, Uri};
    use std::path::PathBuf;
    use std::str::FromStr;

    fn uri(s: &str) -> Uri {
        Uri::from_str(s).expect("valid uri")
    }

    // ---- truncate_doc ----

    #[test]
    fn truncate_doc_short_strings_pass_through() {
        assert_eq!(truncate_doc("hello", 100), "hello");
        assert_eq!(truncate_doc("", 100), "");
    }

    #[test]
    fn truncate_doc_caps_at_max() {
        let s = "a".repeat(SIGNATURE_DOC_MAX_CHARS + 50);
        let out = truncate_doc(&s, SIGNATURE_DOC_MAX_CHARS);
        // The marker is the ellipsis character.
        assert!(out.ends_with('…'));
        // Allow up to one extra byte for the ellipsis.
        assert!(out.len() <= SIGNATURE_DOC_MAX_CHARS + 4);
        // The trimmed payload must be a strict prefix of the input.
        assert!(s.starts_with(out.trim_end_matches('…')));
    }

    #[test]
    fn truncate_doc_respects_utf8_boundaries() {
        // 4-byte emoji at the cut boundary; should not panic.
        let mut s = String::new();
        for _ in 0..(SIGNATURE_DOC_MAX_CHARS / 2) {
            s.push_str("a");
        }
        s.push('🦀');
        s.push_str("rest");
        let _ = truncate_doc(&s, SIGNATURE_DOC_MAX_CHARS);
    }

    // ---- normalize_goto_response ----

    #[test]
    fn normalize_goto_response_link_passthrough() {
        let loc = Location {
            uri: uri("file:///a.rs"),
            range: Range {
                start: Position {
                    line: 1,
                    character: 2,
                },
                end: Position {
                    line: 3,
                    character: 4,
                },
            },
        };
        let resp = GotoDefinitionResponse::Link(vec![LocationLink {
            origin_selection_range: None,
            target_uri: loc.uri.clone(),
            target_range: loc.range,
            target_selection_range: loc.range,
        }]);
        let out = normalize_goto_response(resp);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].target_uri, loc.uri);
        assert_eq!(out[0].target_range, loc.range);
        assert_eq!(out[0].origin_selection_range, None);
    }

    #[test]
    fn normalize_goto_response_scalar_promotes_to_link() {
        let resp = GotoDefinitionResponse::Scalar(Location {
            uri: uri("file:///b.rs"),
            range: Range {
                start: Position {
                    line: 10,
                    character: 0,
                },
                end: Position {
                    line: 10,
                    character: 5,
                },
            },
        });
        let out = normalize_goto_response(resp);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].target_uri, uri("file:///b.rs"));
        assert_eq!(out[0].origin_selection_range, None);
        assert_eq!(out[0].target_selection_range, out[0].target_range);
    }

    #[test]
    fn normalize_goto_response_array_promotes_each_to_link() {
        let resp = GotoDefinitionResponse::Array(vec![
            Location {
                uri: uri("file:///c.rs"),
                range: Range::default(),
            },
            Location {
                uri: uri("file:///d.rs"),
                range: Range::default(),
            },
        ]);
        let out = normalize_goto_response(resp);
        assert_eq!(out.len(), 2);
        assert_eq!(out[0].target_uri, uri("file:///c.rs"));
        assert_eq!(out[1].target_uri, uri("file:///d.rs"));
        for link in &out {
            assert!(link.origin_selection_range.is_none());
        }
    }

    // ---- normalize_workspace_symbol_response ----

    #[test]
    fn normalize_workspace_symbol_response_flat_passthrough() {
        let resp = WorkspaceSymbolResponse::Flat(vec![SymbolInformation {
            name: "foo".to_string(),
            kind: SymbolKind::FUNCTION,
            tags: None,
            location: Location {
                uri: uri("file:///a.rs"),
                range: Range::default(),
            },
            container_name: None,
            #[allow(deprecated)]
            deprecated: None,
        }]);
        let out = normalize_workspace_symbol_response(resp);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].name, "foo");
        assert_eq!(out[0].kind, SymbolKind::FUNCTION);
    }

    #[test]
    fn normalize_workspace_symbol_response_nested_full_location() {
        let resp = WorkspaceSymbolResponse::Nested(vec![WorkspaceSymbol {
            name: "bar".to_string(),
            kind: SymbolKind::STRUCT,
            tags: None,
            container_name: Some("mod".to_string()),
            location: lsp_types::OneOf::Left(Location {
                uri: uri("file:///b.rs"),
                range: Range {
                    start: Position {
                        line: 5,
                        character: 0,
                    },
                    end: Position {
                        line: 5,
                        character: 3,
                    },
                },
            }),
            data: None,
        }]);
        let out = normalize_workspace_symbol_response(resp);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].name, "bar");
        assert_eq!(out[0].kind, SymbolKind::STRUCT);
        assert_eq!(out[0].container_name.as_deref(), Some("mod"));
        assert_eq!(out[0].location.uri, uri("file:///b.rs"));
    }

    #[test]
    fn normalize_workspace_symbol_response_nested_workspace_location() {
        let resp = WorkspaceSymbolResponse::Nested(vec![WorkspaceSymbol {
            name: "baz".to_string(),
            kind: SymbolKind::VARIABLE,
            tags: None,
            container_name: None,
            location: lsp_types::OneOf::Right(WorkspaceLocation {
                uri: uri("file:///c.rs"),
            }),
            data: None,
        }]);
        let out = normalize_workspace_symbol_response(resp);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].name, "baz");
        assert_eq!(out[0].location.uri, uri("file:///c.rs"));
        // WorkspaceLocation has no range → emitted as Range::default().
        assert_eq!(out[0].location.range, Range::default());
    }

    // ---- SignatureHelpSummary ----

    #[test]
    fn signature_help_summary_extracts_simple_label() {
        let help = SignatureHelp {
            signatures: vec![SignatureInformation {
                label: "fn foo(a: i32, b: i32) -> i32".to_string(),
                documentation: Some(Documentation::String("Sums two ints.".to_string())),
                parameters: Some(vec![ParameterInformation {
                    label: ParameterLabel::Simple("a: i32".to_string()),
                    documentation: None,
                }]),
                active_parameter: None,
            }],
            active_signature: Some(0),
            active_parameter: Some(0),
        };
        let summary = SignatureHelpSummary::from_signature_help(&help).unwrap();
        assert_eq!(summary.active_signature, Some(0));
        assert_eq!(summary.active_parameter, Some(0));
        assert_eq!(summary.signatures.len(), 1);
        assert_eq!(summary.signatures[0].label, "fn foo(a: i32, b: i32) -> i32");
        assert_eq!(
            summary.signatures[0].documentation.as_deref(),
            Some("Sums two ints.")
        );
        assert_eq!(summary.signatures[0].parameters.len(), 1);
        assert_eq!(summary.signatures[0].parameters[0].label, "a: i32");
        assert!(summary.signatures[0].parameters[0].documentation.is_none());
    }

    #[test]
    fn signature_help_summary_resolves_label_offsets() {
        // "fn add(x: u32, y: u32) -> u32"
        // Indices:                0123456789012345678901234567
        // Parameter "x: u32" lives at offsets [7, 13) (half-open).
        let help = SignatureHelp {
            signatures: vec![SignatureInformation {
                label: "fn add(x: u32, y: u32) -> u32".to_string(),
                documentation: None,
                parameters: Some(vec![ParameterInformation {
                    label: ParameterLabel::LabelOffsets([7, 13]),
                    documentation: Some(Documentation::String("first".to_string())),
                }]),
                active_parameter: None,
            }],
            active_signature: None,
            active_parameter: None,
        };
        let summary = SignatureHelpSummary::from_signature_help(&help).unwrap();
        assert_eq!(summary.signatures[0].parameters[0].label, "x: u32");
        assert_eq!(
            summary.signatures[0].parameters[0].documentation.as_deref(),
            Some("first")
        );
    }

    #[test]
    fn signature_help_summary_truncates_long_documentation() {
        let huge = "x".repeat(SIGNATURE_DOC_MAX_CHARS * 3);
        let help = SignatureHelp {
            signatures: vec![SignatureInformation {
                label: "fn huge()".to_string(),
                documentation: Some(Documentation::String(huge)),
                parameters: None,
                active_parameter: None,
            }],
            active_signature: None,
            active_parameter: None,
        };
        let summary = SignatureHelpSummary::from_signature_help(&help).unwrap();
        let doc = summary.signatures[0]
            .documentation
            .as_deref()
            .expect("doc present");
        assert!(doc.ends_with('…'));
        assert!(doc.chars().count() <= SIGNATURE_DOC_MAX_CHARS + 1);
    }

    #[test]
    fn signature_help_summary_uses_markup_content_value() {
        let help = SignatureHelp {
            signatures: vec![SignatureInformation {
                label: "fn bar()".to_string(),
                documentation: Some(Documentation::MarkupContent(MarkupContent {
                    kind: MarkupKind::Markdown,
                    value: "**bold** doc".to_string(),
                })),
                parameters: None,
                active_parameter: None,
            }],
            active_signature: None,
            active_parameter: None,
        };
        let summary = SignatureHelpSummary::from_signature_help(&help).unwrap();
        assert_eq!(
            summary.signatures[0].documentation.as_deref(),
            Some("**bold** doc")
        );
    }

    #[test]
    fn signature_help_summary_returns_none_when_empty() {
        let help = SignatureHelp {
            signatures: Vec::new(),
            active_signature: None,
            active_parameter: None,
        };
        assert!(SignatureHelpSummary::from_signature_help(&help).is_none());
    }

    #[test]
    fn signature_help_summary_handles_offset_out_of_bounds() {
        // Offsets deliberately past the end of the label — should
        // produce an empty parameter label rather than panic.
        let help = SignatureHelp {
            signatures: vec![SignatureInformation {
                label: "fn z()".to_string(),
                documentation: None,
                parameters: Some(vec![ParameterInformation {
                    label: ParameterLabel::LabelOffsets([1000, 2000]),
                    documentation: None,
                }]),
                active_parameter: None,
            }],
            active_signature: None,
            active_parameter: None,
        };
        let summary = SignatureHelpSummary::from_signature_help(&help).unwrap();
        assert_eq!(summary.signatures[0].parameters[0].label, "");
    }

    // ---- DocumentHighlight (kind preservation via JSON round-trip) ----

    #[test]
    fn document_highlight_kind_round_trips() {
        // We can't construct an LspOperations without a live service,
        // so exercise the wire shape that the operation depends on.
        let original = vec![
            DocumentHighlight {
                range: Range {
                    start: Position {
                        line: 0,
                        character: 0,
                    },
                    end: Position {
                        line: 0,
                        character: 3,
                    },
                },
                kind: Some(DocumentHighlightKind::TEXT),
            },
            DocumentHighlight {
                range: Range {
                    start: Position {
                        line: 1,
                        character: 0,
                    },
                    end: Position {
                        line: 1,
                        character: 4,
                    },
                },
                kind: Some(DocumentHighlightKind::READ),
            },
            DocumentHighlight {
                range: Range {
                    start: Position {
                        line: 2,
                        character: 0,
                    },
                    end: Position {
                        line: 2,
                        character: 5,
                    },
                },
                kind: Some(DocumentHighlightKind::WRITE),
            },
            DocumentHighlight {
                range: Range {
                    start: Position {
                        line: 3,
                        character: 0,
                    },
                    end: Position {
                        line: 3,
                        character: 1,
                    },
                },
                kind: None,
            },
        ];
        let v = serde_json::to_value(&original).expect("serialize");
        let decoded: Vec<DocumentHighlight> = serde_json::from_value(v).expect("deserialize");
        assert_eq!(decoded, original);
        assert_eq!(decoded[0].kind, Some(DocumentHighlightKind::TEXT));
        assert_eq!(decoded[1].kind, Some(DocumentHighlightKind::READ));
        assert_eq!(decoded[2].kind, Some(DocumentHighlightKind::WRITE));
        assert_eq!(decoded[3].kind, None);
    }

    // ---- capability snapshot decision ----

    #[test]
    fn capability_snapshot_reports_declaration_as_unavailable_when_unset() {
        let caps = ServerCapabilities::default();
        let snap =
            LspCapabilitySnapshot::from_capabilities(&caps, Some("rust-analyzer"), Some("rust"));
        assert!(!snap.supports(LspSemanticOperation::Declaration));
        let u = snap.unavailable(LspSemanticOperation::Declaration).unwrap();
        assert_eq!(u.operation, "declaration");
        assert!(u.reason.contains("rust-analyzer"));
        assert_eq!(u.server.as_deref(), Some("rust-analyzer"));
        assert_eq!(u.language_id.as_deref(), Some("rust"));
    }

    #[test]
    fn capability_snapshot_reports_implementation_as_available_when_set() {
        let mut caps = ServerCapabilities::default();
        caps.implementation_provider = Some(ImplementationProviderCapability::Simple(true));
        let snap =
            LspCapabilitySnapshot::from_capabilities(&caps, Some("rust-analyzer"), Some("rust"));
        assert!(snap.supports(LspSemanticOperation::Implementation));
        assert!(snap
            .unavailable(LspSemanticOperation::Implementation)
            .is_none());
    }

    #[test]
    fn capability_snapshot_reports_document_highlight_as_available_when_set() {
        let mut caps = ServerCapabilities::default();
        caps.document_highlight_provider = Some(OneOf::Left(true));
        let snap =
            LspCapabilitySnapshot::from_capabilities(&caps, Some("tsls"), Some("typescript"));
        assert!(snap.supports(LspSemanticOperation::DocumentHighlight));
    }

    #[test]
    fn capability_snapshot_reports_workspace_symbols_unavailable_when_unset() {
        let caps = ServerCapabilities::default();
        let snap = LspCapabilitySnapshot::from_capabilities(&caps, Some("pylsp"), Some("python"));
        assert!(!snap.supports(LspSemanticOperation::WorkspaceSymbols));
        let u = snap
            .unavailable(LspSemanticOperation::WorkspaceSymbols)
            .unwrap();
        assert_eq!(u.operation, "workspaceSymbol");
        assert!(u.reason.contains("pylsp"));
    }

    // ---- LspUnavailable display ----

    #[test]
    fn lsp_unavailable_display_includes_server_and_language_when_known() {
        let u = LspUnavailable::new(LspSemanticOperation::Declaration, "no provider")
            .with_server("rust-analyzer")
            .with_language_id("rust");
        let s = u.to_string();
        assert!(s.contains("declaration"));
        assert!(s.contains("rust-analyzer"));
        assert!(s.contains("rust"));
        assert!(s.contains("no provider"));
    }

    #[test]
    fn lsp_unavailable_display_falls_back_when_unknown() {
        let u = LspUnavailable::new(LspSemanticOperation::Implementation, "no provider");
        let s = u.to_string();
        assert!(s.contains("implementation"));
        assert!(s.contains("no provider"));
        // No server/language id present.
        assert!(!s.contains("("));
    }

    // ---- format_signature_help_typed ----

    #[test]
    fn format_signature_help_typed_renders_label_and_documentation() {
        let summary = SignatureHelpSummary {
            active_signature: Some(0),
            active_parameter: Some(0),
            signatures: vec![SignatureInfoSummary {
                label: "fn add(a: i32, b: i32) -> i32".to_string(),
                documentation: Some("Adds two ints.".to_string()),
                parameters: vec![
                    SignatureParameterSummary {
                        label: "a: i32".to_string(),
                        documentation: Some("first".to_string()),
                    },
                    SignatureParameterSummary {
                        label: "b: i32".to_string(),
                        documentation: None,
                    },
                ],
            }],
        };
        let out = format_signature_help_typed(&summary);
        assert!(out.contains("fn add(a: i32, b: i32) -> i32"));
        assert!(out.contains("Adds two ints."));
        assert!(out.contains("1. a: i32: first"));
        assert!(out.contains("2. b: i32: "));
    }

    #[test]
    fn format_signature_help_typed_separates_signatures_with_dashes() {
        let summary = SignatureHelpSummary {
            active_signature: None,
            active_parameter: None,
            signatures: vec![
                SignatureInfoSummary {
                    label: "sig1".to_string(),
                    documentation: None,
                    parameters: Vec::new(),
                },
                SignatureInfoSummary {
                    label: "sig2".to_string(),
                    documentation: None,
                    parameters: Vec::new(),
                },
            ],
        };
        let out = format_signature_help_typed(&summary);
        assert!(out.contains("sig1"));
        assert!(out.contains("\n---\n"));
        assert!(out.contains("sig2"));
    }

    // ---- CompletionCandidate ----

    fn completion_item(label: &str, kind: CompletionItemKind) -> CompletionItem {
        CompletionItem {
            label: label.to_string(),
            kind: Some(kind),
            ..Default::default()
        }
    }

    #[test]
    fn completion_candidate_preserves_label_kind_sort_filter() {
        let mut item = completion_item("foo", CompletionItemKind::FUNCTION);
        item.detail = Some("fn foo()".to_string());
        item.sort_text = Some("0001".to_string());
        item.filter_text = Some("foo".to_string());
        item.insert_text = Some("foo()".to_string());

        let cand = CompletionCandidate::from_completion_item(&item);
        assert_eq!(cand.label, "foo");
        assert_eq!(cand.kind.as_deref(), Some("function"));
        assert_eq!(cand.detail.as_deref(), Some("fn foo()"));
        assert_eq!(cand.sort_text.as_deref(), Some("0001"));
        assert_eq!(cand.filter_text.as_deref(), Some("foo"));
        assert_eq!(cand.insert_text_preview.as_deref(), Some("foo()"));
        assert!(!cand.deprecated);
    }

    #[test]
    fn completion_candidate_truncates_long_detail() {
        let mut item = completion_item("foo", CompletionItemKind::FUNCTION);
        item.detail = Some("x".repeat(COMPLETION_DETAIL_MAX_CHARS * 2));
        let cand = CompletionCandidate::from_completion_item(&item);
        let detail = cand.detail.expect("detail present");
        assert!(detail.ends_with('…'));
        assert!(detail.chars().count() <= COMPLETION_DETAIL_MAX_CHARS + 1);
    }

    #[test]
    fn completion_candidate_truncates_long_insert_text() {
        let mut item = completion_item("foo", CompletionItemKind::FUNCTION);
        item.insert_text = Some("y".repeat(COMPLETION_DETAIL_MAX_CHARS * 2));
        let cand = CompletionCandidate::from_completion_item(&item);
        let preview = cand.insert_text_preview.expect("preview present");
        assert!(preview.ends_with('…'));
        assert!(preview.chars().count() <= COMPLETION_DETAIL_MAX_CHARS + 1);
    }

    #[test]
    fn completion_candidate_deprecated_defaults_false_and_respects_true() {
        let item_none = completion_item("a", CompletionItemKind::FUNCTION);
        let cand_none = CompletionCandidate::from_completion_item(&item_none);
        assert!(!cand_none.deprecated);

        let mut item_true = completion_item("b", CompletionItemKind::FUNCTION);
        item_true.deprecated = Some(true);
        let cand_true = CompletionCandidate::from_completion_item(&item_true);
        assert!(cand_true.deprecated);

        let mut item_false = completion_item("c", CompletionItemKind::FUNCTION);
        item_false.deprecated = Some(false);
        let cand_false = CompletionCandidate::from_completion_item(&item_false);
        assert!(!cand_false.deprecated);
    }

    #[test]
    fn completion_bounded_truncates_to_max_candidates_preserving_order() {
        // The same `.iter().take(max).map(...).collect()` pipeline that
        // `completion_bounded` runs after the LSP call. Exercises the
        // bound + server-order preservation invariant.
        let items: Vec<CompletionItem> = vec![
            completion_item("alpha", CompletionItemKind::FUNCTION),
            completion_item("beta", CompletionItemKind::VARIABLE),
            completion_item("gamma", CompletionItemKind::CLASS),
            completion_item("delta", CompletionItemKind::METHOD),
            completion_item("epsilon", CompletionItemKind::ENUM),
        ];
        let max_candidates = 3;
        let bounded: Vec<CompletionCandidate> = items
            .iter()
            .take(max_candidates)
            .map(CompletionCandidate::from_completion_item)
            .collect();
        assert_eq!(bounded.len(), 3);
        // Server order is preserved verbatim — no client-side sort.
        assert_eq!(bounded[0].label, "alpha");
        assert_eq!(bounded[1].label, "beta");
        assert_eq!(bounded[2].label, "gamma");
        assert_eq!(bounded[0].kind.as_deref(), Some("function"));
        assert_eq!(bounded[1].kind.as_deref(), Some("variable"));
        assert_eq!(bounded[2].kind.as_deref(), Some("class"));
    }

    #[test]
    fn completion_bounded_max_candidates_zero_yields_empty() {
        let items = vec![
            completion_item("alpha", CompletionItemKind::FUNCTION),
            completion_item("beta", CompletionItemKind::VARIABLE),
        ];
        let bounded: Vec<CompletionCandidate> = items
            .iter()
            .take(0)
            .map(CompletionCandidate::from_completion_item)
            .collect();
        assert!(bounded.is_empty());
    }

    #[test]
    fn completion_bounded_max_candidates_larger_than_items_returns_all() {
        let items = vec![completion_item("alpha", CompletionItemKind::FUNCTION)];
        let bounded: Vec<CompletionCandidate> = items
            .iter()
            .take(100)
            .map(CompletionCandidate::from_completion_item)
            .collect();
        assert_eq!(bounded.len(), 1);
    }

    // ---- completion_kind_to_string ----

    #[test]
    fn completion_kind_to_string_maps_known_kinds_lowercase() {
        assert_eq!(
            completion_kind_to_string(CompletionItemKind::FUNCTION),
            "function"
        );
        assert_eq!(
            completion_kind_to_string(CompletionItemKind::VARIABLE),
            "variable"
        );
        assert_eq!(
            completion_kind_to_string(CompletionItemKind::ENUM_MEMBER),
            "enum_member"
        );
        assert_eq!(
            completion_kind_to_string(CompletionItemKind::TYPE_PARAMETER),
            "type_parameter"
        );
        assert_eq!(completion_kind_to_string(CompletionItemKind::TEXT), "text");
    }

    #[test]
    fn completion_kind_to_string_falls_back_for_custom_kind() {
        // Custom / unknown integer kind (e.g. server-defined extension)
        // must NOT crash — it should render as `kind(...)` so the
        // surface stays informative. We construct the unknown kind via
        // serde deserialization because the tuple field is private.
        let custom: CompletionItemKind =
            serde_json::from_str("9999").expect("deserialize custom kind");
        let s = completion_kind_to_string(custom);
        assert!(s.starts_with("kind("));
        assert!(s.ends_with(')'));
    }

    // ---- capability gating ----

    #[test]
    fn capability_snapshot_reports_completion_as_unavailable_when_unset() {
        let caps = ServerCapabilities::default();
        let snap = LspCapabilitySnapshot::from_capabilities(&caps, Some("pylsp"), Some("python"));
        assert!(!snap.supports(LspSemanticOperation::Completion));
        let u = snap
            .unavailable(LspSemanticOperation::Completion)
            .expect("unavailable");
        assert_eq!(u.operation, "completion");
        assert!(u.reason.contains("pylsp"));
    }

    #[test]
    fn capability_snapshot_reports_semantic_tokens_as_unavailable_when_unset() {
        let caps = ServerCapabilities::default();
        let snap = LspCapabilitySnapshot::from_capabilities(&caps, Some("pylsp"), Some("python"));
        assert!(!snap.supports(LspSemanticOperation::SemanticTokens));
        let u = snap
            .unavailable(LspSemanticOperation::SemanticTokens)
            .expect("unavailable");
        assert_eq!(u.operation, "semanticTokens");
        assert!(u.reason.contains("pylsp"));
    }

    // ---- decode_semantic_tokens ----

    fn legend(types: &[&str], modifiers: &[&str]) -> SemanticTokenLegendSnapshot {
        SemanticTokenLegendSnapshot {
            token_types: types.iter().map(|s| s.to_string()).collect(),
            token_modifiers: modifiers.iter().map(|s| s.to_string()).collect(),
        }
    }

    #[test]
    fn decode_semantic_tokens_empty_returns_empty_vec() {
        let tokens: Vec<SemanticToken> = Vec::new();
        let l = legend(&["function", "variable"], &["declaration", "deprecated"]);
        let decoded = decode_semantic_tokens(&tokens, &l).expect("decode");
        assert!(decoded.is_empty());
    }

    #[test]
    fn decode_semantic_tokens_single_token_uses_absolute_deltas() {
        // First token: line = delta_line, start = delta_start.
        let tokens = vec![SemanticToken {
            delta_line: 5,
            delta_start: 12,
            length: 4,
            token_type: 0,
            token_modifiers_bitset: 0,
        }];
        let l = legend(&["function"], &[]);
        let decoded = decode_semantic_tokens(&tokens, &l).expect("decode");
        assert_eq!(decoded.len(), 1);
        assert_eq!(decoded[0].line, 5);
        assert_eq!(decoded[0].start, 12);
        assert_eq!(decoded[0].length, 4);
        assert_eq!(decoded[0].token_type, "function");
        assert!(decoded[0].modifiers.is_empty());
    }

    #[test]
    fn decode_semantic_tokens_multiple_on_same_line_accumulates_start() {
        // First token starts at column 3 on line 10. Second token is on
        // the same line, delta_start=4 → start = 3+4 = 7.
        let tokens = vec![
            SemanticToken {
                delta_line: 10,
                delta_start: 3,
                length: 5,
                token_type: 0,
                token_modifiers_bitset: 0,
            },
            SemanticToken {
                delta_line: 0,
                delta_start: 4,
                length: 2,
                token_type: 1,
                token_modifiers_bitset: 0,
            },
        ];
        let l = legend(&["function", "variable"], &[]);
        let decoded = decode_semantic_tokens(&tokens, &l).expect("decode");
        assert_eq!(decoded.len(), 2);
        assert_eq!(decoded[0].line, 10);
        assert_eq!(decoded[0].start, 3);
        assert_eq!(decoded[1].line, 10);
        assert_eq!(decoded[1].start, 7);
        assert_eq!(decoded[1].token_type, "variable");
    }

    #[test]
    fn decode_semantic_tokens_multiple_on_different_lines_uses_absolute_start() {
        // First token: line 4 col 8. Second token: delta_line=2 →
        // line 6, delta_start=1 → start = 1 (absolute on line 6).
        let tokens = vec![
            SemanticToken {
                delta_line: 4,
                delta_start: 8,
                length: 3,
                token_type: 0,
                token_modifiers_bitset: 0,
            },
            SemanticToken {
                delta_line: 2,
                delta_start: 1,
                length: 6,
                token_type: 0,
                token_modifiers_bitset: 0,
            },
        ];
        let l = legend(&["function"], &[]);
        let decoded = decode_semantic_tokens(&tokens, &l).expect("decode");
        assert_eq!(decoded.len(), 2);
        assert_eq!(decoded[0].line, 4);
        assert_eq!(decoded[0].start, 8);
        assert_eq!(decoded[1].line, 6);
        assert_eq!(decoded[1].start, 1);
    }

    #[test]
    fn decode_semantic_tokens_resolves_modifier_bitset() {
        // legend has 3 modifiers. Bits 0 and 2 set → "declaration" and "deprecated".
        let tokens = vec![SemanticToken {
            delta_line: 0,
            delta_start: 0,
            length: 1,
            token_type: 0,
            token_modifiers_bitset: 0b101,
        }];
        let l = legend(&["function"], &["declaration", "readonly", "deprecated"]);
        let decoded = decode_semantic_tokens(&tokens, &l).expect("decode");
        assert_eq!(decoded[0].modifiers, vec!["declaration", "deprecated"]);
    }

    #[test]
    fn decode_semantic_tokens_out_of_range_token_type_returns_structured_error() {
        // Legend has 2 types (indices 0..=1). token_type=2 is out of
        // range → must return a RequestFailed-shaped error.
        let tokens = vec![SemanticToken {
            delta_line: 0,
            delta_start: 0,
            length: 1,
            token_type: 2,
            token_modifiers_bitset: 0,
        }];
        let l = legend(&["function", "variable"], &[]);
        let err = decode_semantic_tokens(&tokens, &l).expect_err("must fail");
        match err {
            LspError::RequestFailed(msg) => {
                assert!(msg.contains("token_type"));
                assert!(msg.contains("index 2"));
                assert!(msg.contains("2 types"));
            }
            other => panic!("expected RequestFailed, got {other:?}"),
        }
    }

    #[test]
    fn decode_semantic_tokens_later_out_of_range_still_returns_error() {
        // The first token is valid; the second token is out of range.
        // We still fail loudly rather than silently dropping the bad
        // token — that's the documented structured-fallback contract.
        let tokens = vec![
            SemanticToken {
                delta_line: 0,
                delta_start: 0,
                length: 1,
                token_type: 0,
                token_modifiers_bitset: 0,
            },
            SemanticToken {
                delta_line: 0,
                delta_start: 2,
                length: 1,
                token_type: 7,
                token_modifiers_bitset: 0,
            },
        ];
        let l = legend(&["function"], &[]);
        assert!(decode_semantic_tokens(&tokens, &l).is_err());
    }

    #[test]
    fn decode_semantic_tokens_three_token_chain_accumulates_correctly() {
        // Realistic LSP stream: 3 tokens, second same line, third on new line.
        // Token A: line=2 col=4 length=3 type=function
        // Token B: delta_line=0 delta_start=5 → line=2 col=9
        // Token C: delta_line=1 delta_start=0 → line=3 col=0
        let tokens = vec![
            SemanticToken {
                delta_line: 2,
                delta_start: 4,
                length: 3,
                token_type: 0,
                token_modifiers_bitset: 0,
            },
            SemanticToken {
                delta_line: 0,
                delta_start: 5,
                length: 4,
                token_type: 1,
                token_modifiers_bitset: 0,
            },
            SemanticToken {
                delta_line: 1,
                delta_start: 0,
                length: 6,
                token_type: 0,
                token_modifiers_bitset: 0,
            },
        ];
        let l = legend(&["function", "variable"], &[]);
        let decoded = decode_semantic_tokens(&tokens, &l).expect("decode");
        assert_eq!(decoded.len(), 3);
        assert_eq!((decoded[0].line, decoded[0].start), (2, 4));
        assert_eq!((decoded[1].line, decoded[1].start), (2, 9));
        assert_eq!((decoded[2].line, decoded[2].start), (3, 0));
        assert_eq!(decoded[0].token_type, "function");
        assert_eq!(decoded[1].token_type, "variable");
        assert_eq!(decoded[2].token_type, "function");
    }

    // ---- PrepareRenameResult ----

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
    fn prepare_rename_result_from_response_default_behavior() {
        let resp = Some(PrepareRenameResponse::DefaultBehavior {
            default_behavior: true,
        });
        let out = PrepareRenameResult::from_response(resp);
        match out {
            PrepareRenameResult::DefaultBehavior { range: r } => {
                assert_eq!(r, lsp_types::Range::default());
            }
            other => panic!("expected DefaultBehavior, got {other:?}"),
        }
    }

    #[test]
    fn prepare_rename_result_from_response_none_is_default_behavior() {
        let out = PrepareRenameResult::from_response(None);
        match out {
            PrepareRenameResult::DefaultBehavior { range: r } => {
                assert_eq!(r, lsp_types::Range::default());
            }
            other => panic!("expected DefaultBehavior, got {other:?}"),
        }
    }

    #[test]
    fn prepare_rename_result_unavailable_range_accessor() {
        let r = range(7, 0);
        let v = PrepareRenameResult::Range {
            range: r,
            placeholder: None,
        };
        assert_eq!(v.range(), Some(&r));

        let d = PrepareRenameResult::DefaultBehavior {
            range: lsp_types::Range::default(),
        };
        assert!(d.range().is_none());

        let u = PrepareRenameResult::Unavailable(LspUnavailable::new(
            LspSemanticOperation::PrepareRename,
            "no provider",
        ));
        assert!(u.range().is_none());
    }

    // ---- CodeActionSummary ----

    fn command_action(title: &str) -> CodeActionOrCommand {
        CodeActionOrCommand::Command(Command {
            title: title.to_string(),
            command: "rust-analyzer.run".to_string(),
            arguments: None,
        })
    }

    fn code_action_with_edit(
        title: &str,
        kind: Option<CodeActionKind>,
        preferred: bool,
        disabled_reason: Option<&str>,
    ) -> CodeActionOrCommand {
        CodeActionOrCommand::CodeAction(CodeAction {
            title: title.to_string(),
            kind,
            diagnostics: Some(vec![Diagnostic::new_simple(
                Range::default(),
                "unused variable".to_string(),
            )]),
            edit: Some(WorkspaceEdit {
                changes: None,
                document_changes: None,
                change_annotations: None,
            }),
            command: None,
            is_preferred: Some(preferred),
            disabled: disabled_reason.map(|r| CodeActionDisabled {
                reason: r.to_string(),
            }),
            data: None,
        })
    }

    #[test]
    fn code_action_summary_for_command_variant() {
        let s = CodeActionSummary::from_action(&command_action("Run cargo build"));
        assert_eq!(s.title, "Run cargo build");
        assert!(s.kind.is_none());
        assert!(!s.preferred);
        assert!(s.disabled_reason.is_none());
        assert!(!s.has_edit);
        assert!(s.has_command);
        assert!(s.diagnostics.is_empty());
    }

    #[test]
    fn code_action_summary_for_code_action_with_edit() {
        let s = CodeActionSummary::from_action(&code_action_with_edit(
            "Remove unused",
            Some(CodeActionKind::QUICKFIX),
            true,
            None,
        ));
        assert_eq!(s.title, "Remove unused");
        assert_eq!(s.kind.as_deref(), Some("quickfix"));
        assert!(s.preferred);
        assert!(s.disabled_reason.is_none());
        assert!(s.has_edit);
        assert!(!s.has_command);
        assert_eq!(s.diagnostics.len(), 1);
        assert!(s.diagnostics[0].contains("unused variable"));
    }

    #[test]
    fn code_action_summary_for_disabled_code_action() {
        let s = CodeActionSummary::from_action(&code_action_with_edit(
            "Extract fn",
            Some(CodeActionKind::REFACTOR_EXTRACT),
            false,
            Some("cursor not on a function"),
        ));
        assert_eq!(s.kind.as_deref(), Some("refactor.extract"));
        assert!(!s.preferred);
        assert_eq!(
            s.disabled_reason.as_deref(),
            Some("cursor not on a function")
        );
    }

    #[test]
    fn code_action_summary_truncation_at_max_actions_preserves_order() {
        // Build a 5-action list and bound at 3. The summarization
        // path inside `code_action_summaries` uses the same
        // `iter().take(max).map(...)` pattern.
        let actions: Vec<CodeActionOrCommand> = (0..5)
            .map(|i| {
                code_action_with_edit(
                    &format!("act-{i}"),
                    Some(CodeActionKind::REFACTOR_REWRITE),
                    false,
                    None,
                )
            })
            .collect();
        let max = 3usize;
        let bounded: Vec<CodeActionSummary> = actions
            .iter()
            .take(max)
            .map(CodeActionSummary::from_action)
            .collect();
        assert_eq!(bounded.len(), 3);
        assert_eq!(bounded[0].title, "act-0");
        assert_eq!(bounded[1].title, "act-1");
        assert_eq!(bounded[2].title, "act-2");
    }

    #[test]
    fn code_action_summary_max_actions_zero_yields_empty() {
        let actions = vec![code_action_with_edit(
            "a",
            Some(CodeActionKind::QUICKFIX),
            false,
            None,
        )];
        let bounded: Vec<CodeActionSummary> = actions
            .iter()
            .take(0)
            .map(CodeActionSummary::from_action)
            .collect();
        assert!(bounded.is_empty());
    }

    #[test]
    fn code_action_summaries_returns_unavailable_when_capability_missing() {
        // Construct a snapshot with no code-action support and
        // verify the unavailable struct has the expected shape.
        let caps = ServerCapabilities::default();
        let snap = LspCapabilitySnapshot::from_capabilities(&caps, Some("pylsp"), Some("python"));
        assert!(!snap.supports(LspSemanticOperation::CodeAction));
        let u = snap
            .unavailable(LspSemanticOperation::CodeAction)
            .expect("unavailable");
        assert_eq!(u.operation, "codeAction");
        assert!(u.reason.contains("pylsp"));
        assert_eq!(u.server.as_deref(), Some("pylsp"));
        assert_eq!(u.language_id.as_deref(), Some("python"));
    }

    // ---- LspError::CommandOnlyCodeAction ----

    #[test]
    fn lsp_error_command_only_code_action_displays_title() {
        let err = LspError::CommandOnlyCodeAction("Run cargo build".to_string());
        let s = err.to_string();
        assert!(s.contains("command-only"));
        assert!(s.contains("Run cargo build"));
        assert!(!err.is_retryable());
    }

    // ---- format_preview_typed helpers (sha256, bounded diff) ----

    #[test]
    fn sha256_hex_is_64_lowercase_hex_chars() {
        let h = sha256_hex(b"hello world");
        // sha256("hello world") = b94d27b9934d3e08a52e52d7da7dabfac484efe37a5380ee9088f7ace2efcde9
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
        // Build a before/after where the diff is guaranteed to
        // exceed FORMATTING_PREVIEW_MAX_DIFF_BYTES.
        let line = "x".repeat(100);
        let before = format!("{}\n", line);
        let after = format!("{}\n", "y".repeat(100));
        // Pad with many lines so the diff is large.
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
        // The truncated output must not exceed the cap by more
        // than the truncation marker + headers.
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
        // Replace the open brace at col 9 with the new body; the
        // closing brace at col 10 is preserved.
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
        // Pure: the same (before, after) pair produces the same
        // before/after hashes. This is the invariant the typed
        // method relies on to detect on-disk file mutation.
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

    // ---- capability gating: prepare_rename / format ----

    #[test]
    fn capability_snapshot_reports_prepare_rename_unavailable_when_unset() {
        // rename_provider absent → both supports_rename and
        // supports_prepare_rename are false.
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
    fn capability_snapshot_reports_document_formatting_unavailable_when_unset() {
        let caps = ServerCapabilities::default();
        let snap = LspCapabilitySnapshot::from_capabilities(&caps, Some("pylsp"), Some("python"));
        assert!(!snap.supports(LspSemanticOperation::DocumentFormatting));
        let u = snap
            .unavailable(LspSemanticOperation::DocumentFormatting)
            .expect("unavailable");
        assert_eq!(u.operation, "formatting");
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

    #[test]
    fn capability_snapshot_reports_document_formatting_available_when_advertised() {
        let mut caps = ServerCapabilities::default();
        caps.document_formatting_provider = Some(lsp_types::OneOf::Left(true));
        let snap = LspCapabilitySnapshot::from_capabilities(&caps, Some("s"), Some("rust"));
        assert!(snap.supports(LspSemanticOperation::DocumentFormatting));
        assert!(snap
            .unavailable(LspSemanticOperation::DocumentFormatting)
            .is_none());
    }
}

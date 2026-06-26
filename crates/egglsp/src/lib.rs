//! Language Server Protocol client crate.
//!
//! This crate owns the LSP client, server registry, file sync, and
//! diagnostics collection. Codegg wires this behind its native `lsp`
//! tool and config types via `From` impls at the boundary.
//!
//! # Capability, Cache, and Semantic Context
//!
//! Beyond the core LSP client, this crate provides:
//!
//! - [`capability`] — normalized server capability snapshots and
//!   structured [`capability::LspUnavailable`] fallback responses.
//! - [`diagnostics`] — debounced diagnostics collection with
//!   [`diagnostics::LspDiagnosticSnapshot`] freshness metadata.
//! - [`semantic_context`] — reusable [`semantic_context::SemanticContextRequest`]
//!   / [`semantic_context::SemanticContextResponse`] API for
//!   domain-agnostic semantic queries.

pub mod bridges;
pub mod cache;
pub mod capability;
pub mod client;
pub mod compatibility;
pub mod config;
pub mod context;
pub mod context_policy;
pub mod context_renderer;
pub mod degradation_policy;
pub mod diagnostics;
pub mod doctor;
pub mod document_sync;
pub mod download;
pub mod edit;
pub mod error;
pub mod evidence_adapter;
pub mod evidence_collector;
pub mod health;
pub mod hunk_context;
pub mod language;
pub mod launch;
pub mod operations;
pub mod overlay;
pub mod position;
pub mod preview_registry;
pub mod restart;
pub mod root;
pub mod runtime;
pub mod security_context;
pub mod semantic_context;
pub mod server;
pub mod server_request;
pub mod service;
pub mod supervisor;
pub mod tui_summary;
pub mod workflow_recipes;
pub mod writer;

pub use bridges::{
    lsp_packet_to_security_summary, lsp_packet_to_tui_summary, semantic_context_to_lsp_items,
};
pub use capability::{
    CapabilityDecision, LspCapabilitySnapshot, LspSemanticOperation, LspUnavailable,
    SemanticTokenLegendSnapshot,
};
pub use client::{
    ClientTransportSnapshot, DiagnosticCacheEntry, JsonRpcId, LspClient, LspClientHealthSnapshot,
    LspClientOptions, OperationalSummary, ProgressSnapshot, ProtocolShutdownTrace,
};
pub use compatibility::{
    CompatibilityCheckStatus, CompatibilityRequirement, LspCompatibilityCheck,
    LspCompatibilityProfile, LspReadinessPolicy, LspRestartMode, LspRestartPolicy,
    LspServerVersion,
};
pub use config::{LspConfig, LspRule};
pub use context::{
    dedup_context_items, default_budget, enforce_context_budget, rank_context_items,
    AgentContextSource, HunkRange, LineRange, LspContextBudget, LspContextItem, LspContextItemKind,
    LspContextMode, LspContextPacket, LspContextPacketMode, LspContextRequest, LspContextScore,
    LspContextTruncation, LspEvidenceFreshness, LspEvidenceProvenance, LspPreviewArtifact,
    LspRiskMode, PreviewFilePatch,
};
pub use context_policy::{
    resolve_model_tier, LspContextPolicy, LspOperationalState as PolicyLspOperationalState,
    LspTaskRisk, LspUnavailablePolicy, StaleEvidencePolicy, TierResolution, TierSource,
};
pub use context_renderer::model_tier_for_profile;
pub use context_renderer::{render_lsp_context_for_agent, LspContextRenderConfig, ModelTier};
pub use diagnostics::{
    DiagnosticsCollector, DiagnosticsOutput, LspDiagnosticFreshness, LspDiagnosticSnapshot,
    LspDiagnosticSource,
};
pub use doctor::{build_doctor_report, render_doctor_report, LspDoctorReport};
pub use document_sync::{OpenDocumentRegistry, OpenDocumentSnapshot};
pub use error::LspError;
pub use evidence_adapter::{EvidenceOperation, ServiceLspEvidenceProvider};
pub use evidence_collector::{
    collect_context, collect_hunk_context, item_kind_from_severity, make_provenance,
    LspContextError, LspEvidenceProvider,
};
pub use health::{LspObservabilitySnapshot, LspOperationalHealthSnapshot, LspOperationalState};
pub use hunk_context::{
    HunkDescriptor, HunkEvidence, HunkLineRange, HunkSourceNavigationLimits,
    HunkSourceNavigationRequest, HunkSourceNavigationResponse,
};
pub use operations::completion_kind_to_string;
pub use operations::decode_semantic_tokens;
pub use operations::select_source_action_edit;
pub use operations::CompletionCandidate;
pub use operations::DecodedSemanticToken;
pub use operations::LspOperations;
pub use operations::SourceActionPreviewKind;
pub use operations::COMPLETION_DETAIL_MAX_CHARS;
pub use operations::{
    CodeActionPreview, CodeActionSummary, FormattingPreview, PrepareRenameResult, RenamePreview,
    VersionedFileEvidence, CODE_ACTION_SUMMARY_DEFAULT_MAX, FORMATTING_PREVIEW_MAX_DIFF_BYTES,
    RENAME_PREVIEW_MAX_EDITS, RENAME_PREVIEW_MAX_FILES,
};
pub use security_context::{
    build_security_evidence_summary, build_security_lsp_context_request, SecurityEvidenceSummary,
    SecurityRiskTag,
};
pub use semantic_context::{
    SemanticContextCaps, SemanticContextIntent, SemanticContextRequest, SemanticContextResponse,
    SemanticLocation,
};
pub use server_request::{
    dispatch_server_request, DynamicRegistration, DynamicRegistrationState, ServerRequestContext,
    ServerRequestReply,
};
pub use service::{LspService, ReadinessResult};
pub use writer::LspWriter;

pub use edit::{FileEditPreview, TextEditPreview, WorkspaceEditPreview};
pub use launch::LspLaunchSpec;
pub use lsp_types;
pub use overlay::{
    OverlayRestoreToken, OverlaySession, SemanticCheckPreview, SemanticSymbolSummary,
};
pub use position::{lsp_range_to_byte_offsets, lsp_units_to_byte_offset, PositionEncoding};
pub use restart::{
    backoff_delay, restart_client_coordinator, LspClientDescriptor, RestartCompletion,
    RestartLease, RestartLeaseAcquisition, RestartOutcome, RestartShared, RestartTaskControl,
    RestartTaskMap, RestartTrigger, ServicePhase,
};
pub use runtime::{spawn_process_runtime, LspProcessIntent, LspProcessRuntime};
pub use supervisor::{LspProcessExitEvent, StderrRingBuffer};
pub use workflow_recipes::{
    default_settings_for_recipe, execute_hunk_source_navigation, execute_preview_suggestion,
    execute_repair_hunk, execute_repair_local, execute_review_diff, execute_review_file,
    execute_security_review_enriched, HunkSourceNavigationRecipeRequest, LspWorkflowRecipe,
    PreviewSuggestionRequest, RecipeOutcome, RecipeSettings, RepairHunkRequest, RepairLocalRequest,
    ReviewDiffRequest, ReviewFileRequest, SecurityReviewEnrichedRequest,
};

#[cfg(feature = "lsp-test-support")]
#[doc(hidden)]
pub mod test_support;

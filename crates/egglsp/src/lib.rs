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

pub mod capability;
pub mod client;
pub mod compatibility;
pub mod config;
pub mod diagnostics;
pub mod document_sync;
pub mod download;
pub mod edit;
pub mod error;
pub mod health;
pub mod hunk_context;
pub mod language;
pub mod launch;
pub mod operations;
pub mod overlay;
pub mod root;
pub mod semantic_context;
pub mod server;
pub mod server_request;
pub mod service;
pub mod supervisor;
pub mod writer;

pub use capability::{LspCapabilitySnapshot, LspSemanticOperation, LspUnavailable};
pub use client::{
    ClientTransportSnapshot, DiagnosticCacheEntry, JsonRpcId, LspClient, LspClientHealthSnapshot,
    LspClientOptions,
};
pub use compatibility::{
    CompatibilityCheckStatus, LspCompatibilityCheck, LspCompatibilityProfile, LspReadinessPolicy,
    LspRestartMode, LspRestartPolicy, LspServerVersion,
};
pub use config::{LspConfig, LspRule};
pub use diagnostics::{
    DiagnosticsCollector, DiagnosticsOutput, LspDiagnosticFreshness, LspDiagnosticSnapshot,
    LspDiagnosticSource,
};
pub use document_sync::{OpenDocumentRegistry, OpenDocumentSnapshot};
pub use error::LspError;
pub use health::{LspOperationalHealthSnapshot, LspOperationalState};
pub use hunk_context::{
    HunkDescriptor, HunkEvidence, HunkLineRange, HunkSourceNavigationLimits,
    HunkSourceNavigationRequest, HunkSourceNavigationResponse,
};
pub use operations::select_source_action_edit;
pub use operations::LspOperations;
pub use operations::SourceActionPreviewKind;
pub use semantic_context::{
    SemanticContextCaps, SemanticContextIntent, SemanticContextRequest, SemanticContextResponse,
    SemanticLocation,
};
pub use server_request::{
    dispatch_server_request, DynamicRegistration, DynamicRegistrationState, ServerRequestContext,
    ServerRequestReply,
};
pub use service::LspService;
pub use writer::LspWriter;

pub use edit::{FileEditPreview, TextEditPreview, WorkspaceEditPreview};
pub use launch::LspLaunchSpec;
pub use lsp_types;
pub use overlay::{
    OverlayRestoreToken, OverlaySession, SemanticCheckPreview, SemanticSymbolSummary,
};
pub use supervisor::{LspProcessExitEvent, StderrRingBuffer};

#[cfg(feature = "lsp-test-support")]
#[doc(hidden)]
pub mod test_support;

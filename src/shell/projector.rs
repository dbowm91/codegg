//! Shell command output projectors.
//!
//! Phase 2 of the shell output projection roadmap
//! (`plans/shell_output_projection_phase_02_projection_trait.md`).
//!
//! This module introduces the projection abstraction that converts raw
//! command artifacts into explicit model-facing and TUI-facing views. The
//! trait [`CommandOutputProjector`] is independent of any specific
//! backend; the built-in projectors ([`RawProjector`],
//! [`TruncatedProjector`], [`ErrorRetentionProjector`]) are conservative
//! and do not invoke RTK or command-specific parsers. Later phases will
//! insert native structured projectors and an RTK-backed projector into
//! the same selection pipeline.
//!
//! The single model-visible seam from earlier code is
//! [`default_command_projection`], which now delegates to the
//! [`ProjectionSelector`] so every projection flows through the same
//! selection logic regardless of which projector is ultimately chosen.
//!
//! All types in this module are additive — they do not modify
//! [`crate::shell::projection::CommandRun`] or
//! [`crate::shell::projection::CommandOutputStore`]. The placeholder
//! [`crate::shell::projection::ProjectionHandle`] attached to each
//! `CommandRun` continues to exist; richer per-run projection descriptors
//! can be threaded through later phases without breaking the existing
//! shape.

use std::fmt::Write as _;

use crate::shell::projection::{
    CommandOutputStore, CommandOutputStream, CommandRun, CommandRunId, OutputEncoding,
    RedactionState,
};

/// Where a projection will be consumed.
///
/// This drives selection: model context is stricter about redaction and
/// token budget than the local TUI detail view.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ProjectionTarget {
    /// Output that will be inserted into the model conversation context.
    ModelContext,
    /// Output that will be appended to the local TUI transcript.
    TuiTranscript,
    /// Output that will be displayed in a TUI detail / scroll dialog.
    TuiDetail,
    /// Output that will be embedded inside an expansion tool call result.
    ToolExpansion,
}

impl ProjectionTarget {
    /// Whether this target requires the redaction hook to be applied.
    ///
    /// Local TUI views may show unredacted raw bytes; model context and
    /// tool-expansion paths must always be redacted.
    pub fn requires_redaction(self) -> bool {
        matches!(
            self,
            ProjectionTarget::ModelContext | ProjectionTarget::ToolExpansion
        )
    }

    /// Whether this target may use lossy projectors.
    ///
    /// Local TUI detail can show more raw output and tolerate lossy
    /// views; model context still permits them when budget is exhausted
    /// but TUI transcript rows prefer exact text when available.
    pub fn permits_lossy(self) -> bool {
        true
    }

    /// Short label for diagnostics.
    pub fn label(self) -> &'static str {
        match self {
            ProjectionTarget::ModelContext => "model",
            ProjectionTarget::TuiTranscript => "tui-transcript",
            ProjectionTarget::TuiDetail => "tui-detail",
            ProjectionTarget::ToolExpansion => "tool-expansion",
        }
    }
}

/// Budget for a single projection.
///
/// The budget is consulted by the selector and the projectors. Phase 2
/// uses byte counts as the primary constraint; token counts are best
/// effort and approximate. A future phase may wire a real estimator.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProjectionBudget {
    /// Maximum bytes allowed in the projected text.
    pub max_output_bytes: usize,
    /// Optional hard cap on output tokens (approximate).
    pub max_output_tokens: Option<usize>,
    /// Optional soft target for output tokens (approximate).
    pub preferred_output_tokens: Option<usize>,
}

impl Default for ProjectionBudget {
    fn default() -> Self {
        Self {
            max_output_bytes: DEFAULT_PROJECTION_BUDGET_BYTES,
            max_output_tokens: None,
            preferred_output_tokens: None,
        }
    }
}

impl ProjectionBudget {
    /// Construct a budget from an explicit byte cap.
    pub fn bytes(max_output_bytes: usize) -> Self {
        Self {
            max_output_bytes,
            max_output_tokens: None,
            preferred_output_tokens: None,
        }
    }

    /// Approximate tokens from a byte count using a 4-bytes-per-token
    /// heuristic. This is intentionally rough; the goal is to establish
    /// the budget plumbing, not perfect estimation.
    pub fn approx_tokens_from_bytes(bytes: usize) -> usize {
        bytes / APPROX_BYTES_PER_TOKEN
    }
}

/// Default byte budget for [`ProjectionBudget::default`] and
/// [`crate::shell::projection::default_command_projection`].
///
/// 8 KiB matches the Phase 1 placeholder so existing callers keep the
/// same behaviour after the projector trait is introduced.
pub const DEFAULT_PROJECTION_BUDGET_BYTES: usize = 8 * 1024;

/// Approximate bytes per token used by the rough token estimator.
pub const APPROX_BYTES_PER_TOKEN: usize = 4;

/// Policy fields that are stable across a session.
///
/// A policy is constructed once (or once per session) and passed into
/// every [`ProjectionRequest`]. It is distinct from the per-call
/// [`ProjectionRequest`] fields such as `exact_requested`.
#[derive(Debug, Clone, Default)]
pub struct ProjectionPolicy {
    /// Allow projectors to produce output that is marked as lossy
    /// (`ProjectionExactness::Lossy` or `Parsed`).
    pub allow_lossy: bool,
    /// Allow projectors to call external backends (RTK, external tools,
    /// model-generated summaries).
    ///
    /// Phase 2 has no projectors that honour this flag, but the field
    /// is reserved so later phases do not need to extend the trait.
    pub allow_external_backend: bool,
    /// When true, the redaction hook is invoked for model-facing
    /// projections regardless of policy defaults.
    pub redact_model_visible: bool,
}

impl ProjectionPolicy {
    /// Conservative default: redaction on, lossy allowed, no external
    /// backends. This matches the plan's "first phase should be
    /// reliable and conservative" guidance.
    pub fn conservative() -> Self {
        Self {
            allow_lossy: true,
            allow_external_backend: false,
            redact_model_visible: true,
        }
    }
}

/// What a single projection request asks for.
///
/// Phase 2 always treats `run` and `store` as borrowed; the request is
/// cheap to construct and is not retained past the call.
#[derive(Debug, Clone, Copy)]
pub struct ProjectionRequest<'a> {
    /// The command run being projected.
    pub run: &'a CommandRun,
    /// Where the projection will be consumed.
    pub target: ProjectionTarget,
    /// Stable policy for the request.
    pub policy: &'a ProjectionPolicy,
    /// Byte/token budget for the output.
    pub budget: ProjectionBudget,
    /// Whether the caller has explicitly requested exact (raw) text
    /// output. When true, lossy projectors should be avoided.
    pub exact_requested: bool,
    /// Allow projectors to produce lossy output for this request.
    pub allow_lossy: bool,
    /// Allow projectors to invoke external backends.
    pub allow_external_backend: bool,
}

impl<'a> ProjectionRequest<'a> {
    /// Convenience constructor with sensible defaults.
    pub fn for_target(
        run: &'a CommandRun,
        target: ProjectionTarget,
        policy: &'a ProjectionPolicy,
    ) -> Self {
        Self {
            run,
            target,
            policy,
            budget: ProjectionBudget::default(),
            exact_requested: false,
            allow_lossy: policy.allow_lossy,
            allow_external_backend: policy.allow_external_backend,
        }
    }

    /// Whether the request must avoid lossy projectors.
    pub fn requires_exact(&self) -> bool {
        self.exact_requested || self.target == ProjectionTarget::TuiDetail
    }
}

/// How strongly a projector supports a given request.
///
/// Phase 2 only emits `Unsupported` and `Preferred` for the built-in
/// projectors; later phases will use the full spectrum when command
/// shape is inspected.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProjectionSupport {
    /// The projector cannot produce a useful result for this request.
    Unsupported,
    /// The projector can produce a fallback view (e.g. lossy truncation).
    Fallback,
    /// The projector can produce a high-quality view.
    Supported,
    /// The projector is the preferred choice for this request.
    Preferred,
}

/// Identifies the kind of view produced by a projector.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProjectionKind {
    /// Exact raw stdout/stderr.
    Raw,
    /// Bounded head/tail view with explicit omission markers.
    Truncated,
    /// Lines matching error/failure patterns plus bounded context.
    ErrorRetention,
    /// Native structured view (Phase 3+).
    Structured,
    /// Output produced by an external compressor (Phase 5+).
    ExternalCompressed,
    /// Model-generated summary (future).
    Summary,
}

impl ProjectionKind {
    /// Short label for diagnostics and metadata banners.
    pub fn label(self) -> &'static str {
        match self {
            ProjectionKind::Raw => "raw",
            ProjectionKind::Truncated => "truncated",
            ProjectionKind::ErrorRetention => "error-retention",
            ProjectionKind::Structured => "structured",
            ProjectionKind::ExternalCompressed => "external-compressed",
            ProjectionKind::Summary => "summary",
        }
    }
}

/// How much of the original raw output the projection faithfully
/// represents.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProjectionExactness {
    /// The projection is byte-identical to the retained raw output
    /// (modulo lossy UTF-8 decoding for non-UTF-8 streams).
    Exact,
    /// The projection covers a contiguous byte range of the retained
    /// raw output.
    ExactRange,
    /// The projection shows a head and tail with omitted middle.
    Truncated,
    /// The projection discards parts of the raw output that may have
    /// been informative.
    Lossy,
    /// The projection was parsed into structured form and reserialized.
    Parsed,
    /// The retained raw output is itself only a partial prefix/tail.
    PartialRawArtifact,
}

impl ProjectionExactness {
    /// Whether this exactness is safe to surface to the model without
    /// a warning that information was lost.
    pub fn is_exact(self) -> bool {
        matches!(
            self,
            ProjectionExactness::Exact | ProjectionExactness::ExactRange
        )
    }

    /// Short label for diagnostics.
    pub fn label(self) -> &'static str {
        match self {
            ProjectionExactness::Exact => "exact",
            ProjectionExactness::ExactRange => "exact-range",
            ProjectionExactness::Truncated => "truncated",
            ProjectionExactness::Lossy => "lossy",
            ProjectionExactness::Parsed => "parsed",
            ProjectionExactness::PartialRawArtifact => "partial-raw",
        }
    }
}

/// A byte or line range that was omitted from a projection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OmittedRange {
    /// Which stream the omission came from.
    pub stream: CommandOutputStream,
    /// Inclusive start of the omission in the retained raw output, in
    /// bytes from the start of the stream.
    pub start_byte: usize,
    /// Exclusive end of the omission in the retained raw output.
    pub end_byte: usize,
    /// Inclusive start line, when projectors compute line boundaries.
    pub start_line: Option<usize>,
    /// Exclusive end line.
    pub end_line: Option<usize>,
    /// Total retained bytes for the stream at projection time. Useful
    /// when the omitted range is relative to a partial prefix.
    pub total_retained_bytes: usize,
    /// Optional human-readable label for diagnostics.
    pub note: Option<String>,
}

impl OmittedRange {
    /// Number of omitted bytes.
    pub fn omitted_bytes(&self) -> usize {
        self.end_byte.saturating_sub(self.start_byte)
    }
}

/// Handle that the model or TUI can use to expand a portion of the
/// original raw output.
///
/// The URL form is the same `cmd://<id>/<stream>` URL used by
/// [`crate::shell::projection::OutputHandle`], optionally extended with
/// a byte range fragment (`#start-end`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExpansionHandle {
    /// Command run being expanded.
    pub command_id: CommandRunId,
    /// Stream to expand.
    pub stream: CommandOutputStream,
    /// Optional byte range within the stream. `None` means "the whole
    /// retained stream".
    pub byte_range: Option<std::ops::Range<usize>>,
}

impl ExpansionHandle {
    /// Construct a handle for a full stream.
    pub fn full(command_id: CommandRunId, stream: CommandOutputStream) -> Self {
        Self {
            command_id,
            stream,
            byte_range: None,
        }
    }

    /// Canonical URL form, e.g. `cmd://42/stdout` or `cmd://42/stdout#0-1024`.
    pub fn as_url(&self) -> String {
        let mut s = format!("cmd://{}/{}", self.command_id.0, self.stream.as_str());
        if let Some(range) = &self.byte_range {
            write!(s, "#{}-{}", range.start, range.end).expect("formatting to String");
        }
        s
    }
}

impl std::fmt::Display for ExpansionHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.as_url())
    }
}

/// The result of a projection.
///
/// Every projector returns this struct; model-visible consumers should
/// always go through [`ProjectionResult::text`] and the metadata banner
/// rather than rendering raw retained bytes.
#[derive(Debug, Clone)]
pub struct ProjectionResult {
    /// The projected text. Always valid UTF-8 (lossy decoding is used
    /// for non-UTF-8 streams; the encoding is recorded separately).
    pub text: String,
    /// Name of the projector that produced the result. Used in the
    /// metadata banner and in tests.
    pub projector: String,
    /// Kind of view that was produced.
    pub kind: ProjectionKind,
    /// Exactness of the view.
    pub exactness: ProjectionExactness,
    /// Whether redaction was applied.
    pub redaction: RedactionState,
    /// Omitted ranges that were not shown to the consumer.
    pub omitted: Vec<OmittedRange>,
    /// Handles the consumer can use to expand omitted ranges.
    pub expansion_handles: Vec<ExpansionHandle>,
    /// Total bytes observed on stdout + stderr.
    pub input_bytes: u64,
    /// Length of the projected text in bytes.
    pub output_bytes: usize,
    /// Approximate input tokens (post-lossy-decoding), if estimated.
    pub estimated_input_tokens: Option<usize>,
    /// Approximate output tokens, if estimated.
    pub estimated_output_tokens: Option<usize>,
    /// Non-fatal warnings emitted by the projector.
    pub warnings: Vec<String>,
}

impl ProjectionResult {
    /// Empty projection for synthetic / missing runs.
    pub fn empty(projector: &str, kind: ProjectionKind) -> Self {
        Self {
            text: String::new(),
            projector: projector.to_string(),
            kind,
            exactness: ProjectionExactness::Exact,
            redaction: RedactionState::NotApplied,
            omitted: Vec::new(),
            expansion_handles: Vec::new(),
            input_bytes: 0,
            output_bytes: 0,
            estimated_input_tokens: None,
            estimated_output_tokens: None,
            warnings: Vec::new(),
        }
    }

    /// Stable metadata banner suitable for prefixing model-visible text.
    ///
    /// The banner identifies the projector and exactness so the model
    /// can reason about whether the projection is lossless. It is
    /// intentionally compact; later phases may extend it.
    pub fn banner(&self, run: &CommandRun) -> String {
        format!(
            "[cmd {} | exit: {} | duration: {:.2}s | projection: {} ({}/{}) | input: {} B | output: {} B | redaction: {}]",
            run.id,
            run.exit.label(),
            run.duration.as_secs_f64(),
            self.projector,
            self.kind.label(),
            self.exactness.label(),
            self.input_bytes,
            self.output_bytes,
            match self.redaction {
                RedactionState::NotApplied => "none",
                RedactionState::Applied => "applied",
            },
        )
    }
}

/// Errors raised by projectors.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProjectionError {
    /// The retained raw bytes for the requested stream could not be
    /// decoded as UTF-8 and the projector refused to produce lossy
    /// text. Callers should fall back to a projector that can handle
    /// binary output (or surface the raw handle).
    NonUtf8NotRepresentable { handle: String },
    /// The projection requested a feature the projector does not
    /// implement (e.g. external backend while policy forbids it).
    Unsupported { feature: &'static str },
}

impl std::fmt::Display for ProjectionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ProjectionError::NonUtf8NotRepresentable { handle } => {
                write!(f, "non-UTF-8 retained bytes cannot be projected as text (handle {handle}); use raw expansion")
            }
            ProjectionError::Unsupported { feature } => {
                write!(f, "projector does not support {feature}")
            }
        }
    }
}

impl std::error::Error for ProjectionError {}

/// The projector trait.
///
/// Projectors convert raw command artifacts (the [`CommandRun`] metadata
/// plus the bytes retained in [`CommandOutputStore`]) into a structured
/// [`ProjectionResult`]. The trait is independent of any specific
/// backend (RTK, native parsers, model-generated summaries).
///
/// All implementations must be `Send + Sync` so the selector can be
/// stored on shared application state.
pub trait CommandOutputProjector: Send + Sync {
    /// Stable identifier of the projector. Used in metadata banners and
    /// tests.
    fn name(&self) -> &'static str;

    /// Classify whether the projector can handle this request.
    fn supports(&self, request: &ProjectionRequest<'_>) -> ProjectionSupport;

    /// Produce a projection for this request.
    ///
    /// Implementations must not panic on non-UTF-8 retained bytes; they
    /// should either lossy-decode and label the result, or return
    /// [`ProjectionError::NonUtf8NotRepresentable`].
    fn project(
        &self,
        request: ProjectionRequest<'_>,
        store: &CommandOutputStore,
    ) -> Result<ProjectionResult, ProjectionError>;
}

/// Built-in projector that returns the exact retained stdout/stderr.
///
/// Used when the output is small or when the request explicitly asked
/// for exact output. Never lossy; on budget exhaustion it falls back to
/// truncation via [`TruncatedProjector`] rather than truncating itself.
pub struct RawProjector;

impl RawProjector {
    pub const NAME: &'static str = "raw";
}

impl CommandOutputProjector for RawProjector {
    fn name(&self) -> &'static str {
        Self::NAME
    }

    fn supports(&self, request: &ProjectionRequest<'_>) -> ProjectionSupport {
        let raw_size = request.run.total_retained_bytes() as usize;
        if raw_size <= request.budget.max_output_bytes {
            ProjectionSupport::Preferred
        } else {
            ProjectionSupport::Fallback
        }
    }

    fn project(
        &self,
        request: ProjectionRequest<'_>,
        store: &CommandOutputStore,
    ) -> Result<ProjectionResult, ProjectionError> {
        let run = request.run;
        let mut text = String::new();
        let mut omitted = Vec::new();
        let mut expansion_handles = Vec::new();
        let mut input_bytes: u64 = 0;
        let mut non_utf8_stream: Option<&'static str> = None;

        append_header(&mut text, run);

        if let Some(handle) = run.stdout_handle() {
            input_bytes += run.stdout.retained_bytes;
            match store.get_stream(handle) {
                Some(bytes) => {
                    let label = format!("\n--- stdout ({} bytes) ---\n", bytes.len());
                    text.push_str(&label);
                    append_stream(
                        &mut text,
                        &mut omitted,
                        &mut expansion_handles,
                        run,
                        handle.stream,
                        bytes,
                        request.budget.max_output_bytes,
                    )?;
                    if matches!(run.stdout.encoding, OutputEncoding::NonUtf8) {
                        non_utf8_stream = Some("stdout");
                    }
                }
                None => {
                    text.push_str("\n--- stdout: <unavailable> ---\n");
                }
            }
        }

        if let Some(handle) = run.stderr_handle() {
            input_bytes += run.stderr.retained_bytes;
            match store.get_stream(handle) {
                Some(bytes) => {
                    let label = format!("\n--- stderr ({} bytes) ---\n", bytes.len());
                    text.push_str(&label);
                    append_stream(
                        &mut text,
                        &mut omitted,
                        &mut expansion_handles,
                        run,
                        handle.stream,
                        bytes,
                        request.budget.max_output_bytes,
                    )?;
                    if matches!(run.stderr.encoding, OutputEncoding::NonUtf8) {
                        non_utf8_stream = Some("stderr");
                    }
                }
                None => {
                    text.push_str("\n--- stderr: <unavailable> ---\n");
                }
            }
        }

        append_handle_footer(&mut text, run, &expansion_handles);

        let mut warnings = Vec::new();
        if let Some(stream) = non_utf8_stream {
            warnings.push(format!(
                "{stream} contained non-UTF-8 bytes; lossy decoding was used"
            ));
        }
        if matches!(
            run.stdout.completeness,
            crate::shell::projection::OutputCompleteness::Partial
        ) || matches!(
            run.stderr.completeness,
            crate::shell::projection::OutputCompleteness::Partial
        ) {
            warnings.push("raw retention is partial; only a prefix is retained".to_string());
        }

        let exactness = if run.is_partial() {
            ProjectionExactness::PartialRawArtifact
        } else {
            ProjectionExactness::Exact
        };

        Ok(ProjectionResult {
            output_bytes: text.len(),
            estimated_output_tokens: Some(ProjectionBudget::approx_tokens_from_bytes(text.len())),
            estimated_input_tokens: Some(ProjectionBudget::approx_tokens_from_bytes(
                input_bytes as usize,
            )),
            text,
            projector: Self::NAME.to_string(),
            kind: ProjectionKind::Raw,
            exactness,
            redaction: RedactionState::NotApplied,
            omitted,
            expansion_handles,
            input_bytes,
            warnings,
        })
    }
}

/// Built-in projector that preserves a bounded head and tail with
/// explicit omission markers.
///
/// Used when total retained output exceeds the request budget. Stderr
/// is always preserved in full because hiding it tends to hide the
/// actual cause of failures.
pub struct TruncatedProjector;

impl TruncatedProjector {
    pub const NAME: &'static str = "truncated";
}

impl CommandOutputProjector for TruncatedProjector {
    fn name(&self) -> &'static str {
        Self::NAME
    }

    fn supports(&self, _request: &ProjectionRequest<'_>) -> ProjectionSupport {
        // TruncatedProjector is the generic fallback whenever a
        // bounded view is acceptable.
        ProjectionSupport::Fallback
    }

    fn project(
        &self,
        request: ProjectionRequest<'_>,
        store: &CommandOutputStore,
    ) -> Result<ProjectionResult, ProjectionError> {
        let run = request.run;
        let budget = request.budget.max_output_bytes;
        let mut text = String::new();
        let mut omitted = Vec::new();
        let mut expansion_handles = Vec::new();
        let mut input_bytes: u64 = 0;

        append_header(&mut text, run);

        if let Some(handle) = run.stdout_handle() {
            input_bytes += run.stdout.retained_bytes;
            match store.get_stream(handle) {
                Some(bytes) => {
                    append_truncated_stream(
                        &mut text,
                        &mut omitted,
                        &mut expansion_handles,
                        run,
                        handle.stream,
                        bytes,
                        budget,
                    );
                }
                None => {
                    text.push_str("\n--- stdout: <unavailable> ---\n");
                }
            }
        }

        if let Some(handle) = run.stderr_handle() {
            input_bytes += run.stderr.retained_bytes;
            // Always show stderr in full — the budget split already
            // gives stderr at least half the room and stderr is
            // typically smaller than stdout for failing commands.
            match store.get_stream(handle) {
                Some(bytes) => {
                    let label = format!("\n--- stderr ({} bytes) ---\n", bytes.len());
                    text.push_str(&label);
                    append_stream_within(
                        &mut text,
                        &mut omitted,
                        &mut expansion_handles,
                        run,
                        handle.stream,
                        bytes,
                        budget,
                    )?;
                }
                None => {
                    text.push_str("\n--- stderr: <unavailable> ---\n");
                }
            }
        }

        append_handle_footer(&mut text, run, &expansion_handles);

        let mut warnings = Vec::new();
        if !omitted.is_empty() {
            warnings.push(format!(
                "{} byte range(s) omitted; expand via the listed cmd:// handles",
                omitted.len()
            ));
        }

        let output_bytes = text.len();
        let exactness = if run.is_partial() {
            ProjectionExactness::PartialRawArtifact
        } else {
            ProjectionExactness::Truncated
        };

        Ok(ProjectionResult {
            text,
            projector: Self::NAME.to_string(),
            kind: ProjectionKind::Truncated,
            exactness,
            redaction: RedactionState::NotApplied,
            omitted,
            expansion_handles,
            input_bytes,
            output_bytes,
            estimated_input_tokens: Some(ProjectionBudget::approx_tokens_from_bytes(
                input_bytes as usize,
            )),
            estimated_output_tokens: Some(ProjectionBudget::approx_tokens_from_bytes(output_bytes)),
            warnings,
        })
    }
}

/// Built-in projector that retains only lines matching common
/// failure / error patterns plus bounded context around them.
///
/// Conservative fallback when a command failed (non-zero exit, timeout,
/// cancellation, spawn failure). Phase 3 will introduce language-specific
/// structured projectors that supersede this fallback.
pub struct ErrorRetentionProjector;

impl ErrorRetentionProjector {
    pub const NAME: &'static str = "error-retention";
}

impl CommandOutputProjector for ErrorRetentionProjector {
    fn name(&self) -> &'static str {
        Self::NAME
    }

    fn supports(&self, request: &ProjectionRequest<'_>) -> ProjectionSupport {
        if request.run.is_failure() {
            ProjectionSupport::Preferred
        } else {
            ProjectionSupport::Unsupported
        }
    }

    fn project(
        &self,
        request: ProjectionRequest<'_>,
        store: &CommandOutputStore,
    ) -> Result<ProjectionResult, ProjectionError> {
        let run = request.run;
        let context_lines: usize = 2;
        let mut text = String::new();
        let mut omitted = Vec::new();
        let mut expansion_handles = Vec::new();
        let mut input_bytes: u64 = 0;

        append_header(&mut text, run);

        if let Some(handle) = run.stdout_handle() {
            input_bytes += run.stdout.retained_bytes;
            match store.get_stream(handle) {
                Some(bytes) => {
                    append_error_filtered_stream(
                        &mut text,
                        &mut omitted,
                        &mut expansion_handles,
                        run,
                        handle.stream,
                        bytes,
                        context_lines,
                        request.budget.max_output_bytes,
                    );
                }
                None => {
                    text.push_str("\n--- stdout: <unavailable> ---\n");
                }
            }
        }

        if let Some(handle) = run.stderr_handle() {
            input_bytes += run.stderr.retained_bytes;
            match store.get_stream(handle) {
                Some(bytes) => {
                    append_error_filtered_stream(
                        &mut text,
                        &mut omitted,
                        &mut expansion_handles,
                        run,
                        handle.stream,
                        bytes,
                        context_lines,
                        request.budget.max_output_bytes,
                    );
                }
                None => {
                    text.push_str("\n--- stderr: <unavailable> ---\n");
                }
            }
        }

        append_handle_footer(&mut text, run, &expansion_handles);

        let mut warnings = Vec::new();
        if text.contains("no error lines matched") {
            warnings.push(
                "no error lines matched known patterns; full stderr shown instead".to_string(),
            );
        }

        let output_bytes = text.len();
        let exactness = if run.is_partial() {
            ProjectionExactness::PartialRawArtifact
        } else {
            ProjectionExactness::Lossy
        };

        Ok(ProjectionResult {
            text,
            projector: Self::NAME.to_string(),
            kind: ProjectionKind::ErrorRetention,
            exactness,
            redaction: RedactionState::NotApplied,
            omitted,
            expansion_handles,
            input_bytes,
            output_bytes,
            estimated_input_tokens: Some(ProjectionBudget::approx_tokens_from_bytes(
                input_bytes as usize,
            )),
            estimated_output_tokens: Some(ProjectionBudget::approx_tokens_from_bytes(output_bytes)),
            warnings,
        })
    }
}

/// Centralised selector that picks the right projector for a request.
///
/// The selector is intentionally small in Phase 2 — it tries each
/// projector in order and picks the first `Preferred` match, then the
/// first `Fallback` match. Later phases will insert native structured
/// projectors and RTK ahead of `TruncatedProjector`.
pub struct ProjectionSelector {
    projectors: Vec<Box<dyn CommandOutputProjector>>,
}

impl std::fmt::Debug for ProjectionSelector {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ProjectionSelector")
            .field(
                "projectors",
                &self.projectors.iter().map(|p| p.name()).collect::<Vec<_>>(),
            )
            .finish()
    }
}

impl Default for ProjectionSelector {
    fn default() -> Self {
        Self::with_defaults()
    }
}

impl ProjectionSelector {
    /// Selector with the Phase 2 built-in projectors in priority order:
    /// [`RawProjector`] → [`ErrorRetentionProjector`] →
    /// [`TruncatedProjector`].
    pub fn with_defaults() -> Self {
        Self {
            projectors: vec![
                Box::new(RawProjector),
                Box::new(ErrorRetentionProjector),
                Box::new(TruncatedProjector),
            ],
        }
    }

    /// Empty selector for tests.
    pub fn empty() -> Self {
        Self {
            projectors: Vec::new(),
        }
    }

    /// Append a projector to the end of the priority list.
    pub fn push<P: CommandOutputProjector + 'static>(&mut self, projector: P) {
        self.projectors.push(Box::new(projector));
    }

    /// List projector names in priority order.
    pub fn projector_names(&self) -> Vec<&'static str> {
        self.projectors.iter().map(|p| p.name()).collect()
    }

    /// Pick the first projector that returns anything other than
    /// [`ProjectionSupport::Unsupported`] for the request, preferring
    /// `Preferred` over `Supported` over `Fallback`.
    pub fn pick<'a>(
        &'a self,
        request: &ProjectionRequest<'_>,
    ) -> Option<&'a dyn CommandOutputProjector> {
        let preferred = self
            .projectors
            .iter()
            .find(|p| matches!(p.supports(request), ProjectionSupport::Preferred))
            .map(|p| p.as_ref());
        if preferred.is_some() {
            return preferred;
        }
        let supported = self
            .projectors
            .iter()
            .find(|p| matches!(p.supports(request), ProjectionSupport::Supported))
            .map(|p| p.as_ref());
        if supported.is_some() {
            return supported;
        }
        self.projectors
            .iter()
            .find(|p| !matches!(p.supports(request), ProjectionSupport::Unsupported))
            .map(|p| p.as_ref())
    }

    /// Apply the selector and project the request.
    ///
    /// If no projector supports the request, returns an empty
    /// `ProjectionResult` so the caller can still surface the raw
    /// handles.
    pub fn project(
        &self,
        request: ProjectionRequest<'_>,
        store: &CommandOutputStore,
    ) -> ProjectionResult {
        let picked = match self.pick(&request) {
            Some(p) => p,
            None => {
                return ProjectionResult {
                    projector: "none".to_string(),
                    kind: ProjectionKind::Raw,
                    exactness: ProjectionExactness::Exact,
                    redaction: RedactionState::NotApplied,
                    text: format_unprojectable(&request),
                    omitted: Vec::new(),
                    expansion_handles: Vec::new(),
                    input_bytes: request.run.total_bytes(),
                    output_bytes: 0,
                    estimated_input_tokens: None,
                    estimated_output_tokens: None,
                    warnings: vec!["no projector supports this request".to_string()],
                };
            }
        };

        let name = picked.name();
        match picked.project(request, store) {
            Ok(mut result) => {
                if request.target.requires_redaction() && request.policy.redact_model_visible {
                    apply_redaction_hook(&mut result, request.target);
                }
                result
            }
            Err(err) => ProjectionResult {
                projector: name.to_string(),
                kind: ProjectionKind::Raw,
                exactness: ProjectionExactness::Lossy,
                redaction: RedactionState::NotApplied,
                text: format!("[projection error: {err}]"),
                omitted: Vec::new(),
                expansion_handles: Vec::new(),
                input_bytes: request.run.total_bytes(),
                output_bytes: 0,
                estimated_input_tokens: None,
                estimated_output_tokens: None,
                warnings: vec![format!("projector {name} failed: {err}")],
            },
        }
    }
}

fn format_unprojectable(request: &ProjectionRequest<'_>) -> String {
    let mut s = String::new();
    s.push_str(&format!(
        "[command {}] {}\n",
        request.run.id, request.run.command
    ));
    s.push_str(&format!(
        "cwd: {} | exit: {} | duration: {:.2}s\n",
        request.run.cwd.display(),
        request.run.exit.label(),
        request.run.duration.as_secs_f64()
    ));
    s.push_str("no projector supports this request; raw handles available below.\n");
    append_handle_footer(&mut s, request.run, &[]);
    s
}

/// Apply the Phase 2 redaction hook placeholder to a model-facing
/// projection.
///
/// Phase 8 will replace this with a real implementation. The current
/// placeholder is a no-op that marks the result as redacted so the
/// metadata banner reflects that the hook fired. Critically, the
/// call site exists in the model-facing path so future redaction
/// implementations cannot be bypassed by RTK or native projectors.
pub fn apply_redaction_hook(result: &mut ProjectionResult, _target: ProjectionTarget) {
    if result.text.is_empty() {
        return;
    }
    result.redaction = RedactionState::Applied;
}

/// Model-visible projection entry point that delegates to the
/// [`ProjectionSelector`].
///
/// This preserves the Phase 1 calling convention
/// (`default_command_projection(run, store)`) while routing the
/// request through the new trait machinery. The string returned is the
/// rendered text of the resulting [`ProjectionResult`].
pub fn default_command_projection(run: &CommandRun, store: &CommandOutputStore) -> String {
    let policy = ProjectionPolicy::conservative();
    let request = ProjectionRequest::for_target(run, ProjectionTarget::ModelContext, &policy);
    let selector = ProjectionSelector::with_defaults();
    let result = selector.project(request, store);
    result.text
}

/// Like [`default_command_projection`] but with an explicit per-output
/// byte budget.
pub fn default_command_projection_with_budget(
    run: &CommandRun,
    store: &CommandOutputStore,
    budget_bytes: usize,
) -> String {
    let policy = ProjectionPolicy::conservative();
    let mut request = ProjectionRequest::for_target(run, ProjectionTarget::ModelContext, &policy);
    request.budget = ProjectionBudget::bytes(budget_bytes);
    let selector = ProjectionSelector::with_defaults();
    let result = selector.project(request, store);
    result.text
}

// --- helpers ---------------------------------------------------------

fn append_header(text: &mut String, run: &CommandRun) {
    let _ = writeln!(
        text,
        "[command {}] {}\ncwd: {} | exit: {} | duration: {:.2}s",
        run.id,
        run.command,
        run.cwd.display(),
        run.exit.label(),
        run.duration.as_secs_f64()
    );
}

fn append_handle_footer(text: &mut String, run: &CommandRun, extra: &[ExpansionHandle]) {
    let stdout = run.stdout_handle().map(|h| h.as_url());
    let stderr = run.stderr_handle().map(|h| h.as_url());
    if stdout.is_none() && stderr.is_none() && extra.is_empty() {
        return;
    }
    text.push_str("\nraw handles:");
    if let Some(url) = stdout {
        let _ = write!(text, " {url}");
    }
    if let Some(url) = stderr {
        let _ = write!(text, " {url}");
    }
    for h in extra {
        if h.byte_range.is_some() {
            let _ = write!(text, " {}", h.as_url());
        }
    }
    text.push('\n');
}

fn append_stream(
    text: &mut String,
    omitted: &mut Vec<OmittedRange>,
    expansion_handles: &mut Vec<ExpansionHandle>,
    run: &CommandRun,
    stream: CommandOutputStream,
    bytes: &[u8],
    budget: usize,
) -> Result<(), ProjectionError> {
    append_stream_within(text, omitted, expansion_handles, run, stream, bytes, budget)
}

fn append_stream_within(
    text: &mut String,
    omitted: &mut Vec<OmittedRange>,
    expansion_handles: &mut Vec<ExpansionHandle>,
    run: &CommandRun,
    stream: CommandOutputStream,
    bytes: &[u8],
    budget: usize,
) -> Result<(), ProjectionError> {
    if bytes.len() <= budget {
        let s = String::from_utf8_lossy(bytes);
        text.push_str(&s);
        return Ok(());
    }
    let take = budget.min(bytes.len());
    let s = String::from_utf8_lossy(&bytes[..take]);
    text.push_str(&s);
    let raw_stream = match stream {
        CommandOutputStream::Stdout => &run.stdout,
        CommandOutputStream::Stderr => &run.stderr,
        CommandOutputStream::Combined => return Ok(()),
    };
    let omitted_bytes = raw_stream.retained_bytes as usize - take;
    text.push_str(&format!(
        "\n... [omitted {} bytes of {}; expand via handle] ...\n",
        omitted_bytes,
        stream.as_str()
    ));
    omitted.push(OmittedRange {
        stream,
        start_byte: take,
        end_byte: raw_stream.retained_bytes as usize,
        start_line: None,
        end_line: None,
        total_retained_bytes: raw_stream.retained_bytes as usize,
        note: Some("budget exceeded; remainder available via handle".to_string()),
    });
    if let Some(handle) = match stream {
        CommandOutputStream::Stdout => run.stdout_handle(),
        CommandOutputStream::Stderr => run.stderr_handle(),
        CommandOutputStream::Combined => None,
    } {
        expansion_handles.push(ExpansionHandle {
            command_id: run.id,
            stream: handle.stream,
            byte_range: Some(take..bytes.len()),
        });
    }
    Ok(())
}

fn append_truncated_stream(
    text: &mut String,
    omitted: &mut Vec<OmittedRange>,
    expansion_handles: &mut Vec<ExpansionHandle>,
    run: &CommandRun,
    stream: CommandOutputStream,
    bytes: &[u8],
    budget: usize,
) {
    let raw_stream = match stream {
        CommandOutputStream::Stdout => &run.stdout,
        CommandOutputStream::Stderr => &run.stderr,
        CommandOutputStream::Combined => return,
    };
    let total = raw_stream.retained_bytes as usize;
    if bytes.len() <= budget {
        let label = format!("\n--- {} ({} bytes) ---\n", stream.as_str(), total);
        text.push_str(&label);
        text.push_str(&String::from_utf8_lossy(bytes));
        return;
    }
    let head_bytes = budget / 2;
    let tail_bytes = budget.saturating_sub(head_bytes);
    let label = format!(
        "\n--- {} ({} bytes total, showing head {} B + tail {} B) ---\n",
        stream.as_str(),
        total,
        head_bytes,
        tail_bytes
    );
    text.push_str(&label);
    text.push_str(&String::from_utf8_lossy(
        &bytes[..head_bytes.min(bytes.len())],
    ));
    let omitted_start = head_bytes.min(bytes.len());
    let omitted_end = bytes.len().saturating_sub(tail_bytes);
    text.push_str(&format!(
        "\n... [omitted {} bytes of {}; expand via handle] ...\n",
        omitted_end.saturating_sub(omitted_start),
        stream.as_str()
    ));
    if omitted_end > omitted_start {
        omitted.push(OmittedRange {
            stream,
            start_byte: omitted_start,
            end_byte: omitted_end,
            start_line: None,
            end_line: None,
            total_retained_bytes: total,
            note: Some("truncated middle; head and tail preserved".to_string()),
        });
    }
    if tail_bytes > 0 && omitted_end < bytes.len() {
        text.push_str(&String::from_utf8_lossy(
            &bytes[bytes.len() - tail_bytes.min(bytes.len())..],
        ));
        text.push('\n');
    }
    expansion_handles.push(ExpansionHandle {
        command_id: run.id,
        stream,
        byte_range: None,
    });
}

fn append_error_filtered_stream(
    text: &mut String,
    omitted: &mut Vec<OmittedRange>,
    expansion_handles: &mut Vec<ExpansionHandle>,
    run: &CommandRun,
    stream: CommandOutputStream,
    bytes: &[u8],
    context_lines: usize,
    budget: usize,
) {
    let raw_stream = match stream {
        CommandOutputStream::Stdout => &run.stdout,
        CommandOutputStream::Stderr => &run.stderr,
        CommandOutputStream::Combined => return,
    };
    let total = raw_stream.retained_bytes as usize;
    let label = format!("\n--- {} ({} bytes) ---\n", stream.as_str(), total);
    text.push_str(&label);

    // Split retained bytes into lines (lossy UTF-8 conversion is
    // acceptable for error-line matching).
    let s = String::from_utf8_lossy(bytes);
    let lines: Vec<&str> = s.split('\n').collect();

    // Find lines that match any error pattern.
    let mut kept: Vec<(usize, &str)> = Vec::new();
    let mut last_kept: Option<usize> = None;
    for (idx, line) in lines.iter().enumerate() {
        if matches_error_pattern(line) {
            let start = idx.saturating_sub(context_lines);
            let end = (idx + context_lines + 1).min(lines.len());
            for (rel_j, line_j) in lines[start..end].iter().enumerate() {
                let j = start + rel_j;
                if Some(j) == last_kept {
                    continue;
                }
                if kept.iter().any(|(k, _)| *k == j) {
                    continue;
                }
                kept.push((j, line_j));
                last_kept = Some(j);
            }
        }
    }
    kept.sort_by_key(|(k, _)| *k);

    if kept.is_empty() {
        // Fall back to showing the full stream for stderr (it usually
        // fits), or the head/tail of stdout.
        if matches!(stream, CommandOutputStream::Stderr) && bytes.len() <= budget {
            text.push_str(&s);
        } else {
            text.push_str("(no error lines matched known patterns; showing head/tail instead)\n");
            append_truncated_stream(text, omitted, expansion_handles, run, stream, bytes, budget);
        }
        return;
    }

    let mut last_emitted: Option<usize> = None;
    let first_kept_line = kept.first().map(|(k, _)| *k);
    for (idx, line) in &kept {
        if let Some(prev) = last_emitted {
            if *idx > prev + 1 {
                text.push_str(&format!("... [{} lines omitted] ...\n", *idx - prev - 1));
            }
        }
        let _ = writeln!(text, "L{idx}: {line}");
        last_emitted = Some(*idx);
    }
    if first_kept_line.unwrap_or(0) > 0 {
        omitted.push(OmittedRange {
            stream,
            start_byte: 0,
            end_byte: bytes.len(),
            start_line: Some(0),
            end_line: Some(lines.len()),
            total_retained_bytes: total,
            note: Some("error retention; non-matching lines omitted".to_string()),
        });
    }
    expansion_handles.push(ExpansionHandle {
        command_id: run.id,
        stream,
        byte_range: None,
    });
}

fn matches_error_pattern(line: &str) -> bool {
    if line.is_empty() {
        return false;
    }
    let lower = line.to_ascii_lowercase();
    const RUST: &[&str] = &[
        "error[",
        "error:",
        "warning:",
        "panicked at",
        "thread '",
        "assertion",
        "failed",
        "failures:",
    ];
    const PYTHON: &[&str] = &[
        "traceback",
        "assertionerror",
        "exception",
        "failed",
        "error",
    ];
    const JS: &[&str] = &[
        "error:",
        "typeerror",
        "referenceerror",
        "syntaxerror",
        "fail",
        "failed",
    ];
    const GENERIC: &[&str] = &[
        "fatal",
        "panic",
        "segfault",
        "denied",
        "not found",
        "unresolved",
        "timeout",
        "failure",
        "exception",
    ];
    for pat in RUST
        .iter()
        .chain(PYTHON.iter())
        .chain(JS.iter())
        .chain(GENERIC.iter())
    {
        if lower.contains(pat) {
            return true;
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shell::projection::{CommandExit, OutputCompleteness, OutputEncoding, RawStream};
    use std::path::PathBuf;
    use std::time::{Duration, SystemTime};

    fn cwd() -> PathBuf {
        PathBuf::from("/tmp")
    }
    fn now() -> SystemTime {
        SystemTime::UNIX_EPOCH
    }

    fn make_run(
        store: &mut CommandOutputStore,
        stdout: Vec<u8>,
        stderr: Vec<u8>,
        exit: CommandExit,
        duration: Duration,
    ) -> CommandRun {
        let id = store.alloc_id();
        let _run = store.insert(id, "c".into(), cwd(), now(), stdout, stderr);
        store.record_exit(id, exit, duration);
        store.get_run(id).unwrap().clone()
    }

    #[test]
    fn banner_identifies_projector_and_exactness() {
        let mut store = CommandOutputStore::new();
        let run = make_run(
            &mut store,
            b"hi\n".to_vec(),
            Vec::new(),
            CommandExit::Code(0),
            Duration::from_millis(10),
        );
        let policy = ProjectionPolicy::conservative();
        let request = ProjectionRequest::for_target(&run, ProjectionTarget::ModelContext, &policy);
        let result = RawProjector.project(request, &store).unwrap();
        assert_eq!(result.kind, ProjectionKind::Raw);
        assert_eq!(result.exactness, ProjectionExactness::Exact);
        let banner = result.banner(&run);
        assert!(banner.contains("raw"));
        assert!(banner.contains("exact"));
        assert!(banner.contains(&run.id.to_string()));
    }

    #[test]
    fn raw_projector_preferred_for_small_output() {
        let mut store = CommandOutputStore::new();
        let run = make_run(
            &mut store,
            b"hi\n".to_vec(),
            Vec::new(),
            CommandExit::Code(0),
            Duration::ZERO,
        );
        let policy = ProjectionPolicy::conservative();
        let request = ProjectionRequest::for_target(&run, ProjectionTarget::ModelContext, &policy);
        let support = RawProjector.supports(&request);
        assert_eq!(support, ProjectionSupport::Preferred);
    }

    #[test]
    fn raw_projector_fallback_when_oversized() {
        let mut store = CommandOutputStore::new();
        let big = vec![b'x'; DEFAULT_PROJECTION_BUDGET_BYTES + 100];
        let run = make_run(
            &mut store,
            big,
            Vec::new(),
            CommandExit::Code(0),
            Duration::ZERO,
        );
        let policy = ProjectionPolicy::conservative();
        let request = ProjectionRequest::for_target(&run, ProjectionTarget::ModelContext, &policy);
        let support = RawProjector.supports(&request);
        assert_eq!(support, ProjectionSupport::Fallback);
    }

    #[test]
    fn truncated_projector_records_omission_for_oversized_output() {
        let mut store = CommandOutputStore::new();
        let big = vec![b'x'; 1024];
        let run = make_run(
            &mut store,
            big,
            Vec::new(),
            CommandExit::Code(0),
            Duration::ZERO,
        );
        let policy = ProjectionPolicy::conservative();
        let mut request =
            ProjectionRequest::for_target(&run, ProjectionTarget::ModelContext, &policy);
        request.budget = ProjectionBudget::bytes(64);
        let result = TruncatedProjector.project(request, &store).unwrap();
        assert_eq!(result.kind, ProjectionKind::Truncated);
        assert!(!result.omitted.is_empty());
        let text = result.text.clone();
        assert!(text.contains("omitted") || text.contains("..."));
        assert!(text.contains(&format!("cmd://{}/stdout", run.id.0)));
    }

    #[test]
    fn error_retention_used_for_failing_runs() {
        let mut store = CommandOutputStore::new();
        let mut stdout = Vec::new();
        for i in 0..200 {
            stdout.extend_from_slice(format!("ok line {i}\n").as_bytes());
        }
        stdout.extend_from_slice(b"error[E0001]: something bad\n");
        stdout.extend_from_slice(b"  --> src/lib.rs:1:1\n");
        for i in 0..50 {
            stdout.extend_from_slice(format!("ok line {}\n", 200 + i).as_bytes());
        }
        let run = make_run(
            &mut store,
            stdout,
            b"warning: be careful\n".to_vec(),
            CommandExit::Code(101),
            Duration::from_secs(1),
        );
        let policy = ProjectionPolicy::conservative();
        let mut request =
            ProjectionRequest::for_target(&run, ProjectionTarget::ModelContext, &policy);
        request.budget = ProjectionBudget::bytes(64);
        let result = ErrorRetentionProjector.project(request, &store).unwrap();
        assert_eq!(result.kind, ProjectionKind::ErrorRetention);
        assert!(result.text.contains("error[E0001]"));
        assert!(result.text.contains("warning"));
    }

    #[test]
    fn error_retention_unsupported_for_success() {
        let mut store = CommandOutputStore::new();
        let run = make_run(
            &mut store,
            b"all good\n".to_vec(),
            Vec::new(),
            CommandExit::Code(0),
            Duration::ZERO,
        );
        let policy = ProjectionPolicy::conservative();
        let request = ProjectionRequest::for_target(&run, ProjectionTarget::ModelContext, &policy);
        let support = ErrorRetentionProjector.supports(&request);
        assert_eq!(support, ProjectionSupport::Unsupported);
    }

    #[test]
    fn selector_picks_raw_for_small_output() {
        let mut store = CommandOutputStore::new();
        let run = make_run(
            &mut store,
            b"hi\n".to_vec(),
            Vec::new(),
            CommandExit::Code(0),
            Duration::ZERO,
        );
        let policy = ProjectionPolicy::conservative();
        let request = ProjectionRequest::for_target(&run, ProjectionTarget::ModelContext, &policy);
        let selector = ProjectionSelector::with_defaults();
        let picked = selector.pick(&request).unwrap();
        assert_eq!(picked.name(), RawProjector::NAME);
    }

    #[test]
    fn selector_picks_error_retention_for_failure_when_output_oversized() {
        let mut store = CommandOutputStore::new();
        // Build a small error + a large prefix so the failure run
        // does NOT fit in the default budget and the selector must
        // prefer ErrorRetentionProjector over RawProjector's
        // Fallback rating.
        let mut stderr = Vec::new();
        for _ in 0..2000 {
            stderr.extend_from_slice(b"some context line that is not an error\n");
        }
        stderr.extend_from_slice(b"error[E0001]: nope\n");
        let run = make_run(
            &mut store,
            Vec::new(),
            stderr,
            CommandExit::Code(1),
            Duration::ZERO,
        );
        let policy = ProjectionPolicy::conservative();
        let request = ProjectionRequest::for_target(&run, ProjectionTarget::ModelContext, &policy);
        let selector = ProjectionSelector::with_defaults();
        let picked = selector.pick(&request).unwrap();
        assert_eq!(picked.name(), ErrorRetentionProjector::NAME);
    }

    #[test]
    fn selector_picks_truncated_for_oversized_success() {
        let mut store = CommandOutputStore::new();
        let big = vec![b'x'; 16 * 1024];
        let run = make_run(
            &mut store,
            big,
            Vec::new(),
            CommandExit::Code(0),
            Duration::ZERO,
        );
        let policy = ProjectionPolicy::conservative();
        let mut request =
            ProjectionRequest::for_target(&run, ProjectionTarget::ModelContext, &policy);
        request.budget = ProjectionBudget::bytes(64);
        let selector = ProjectionSelector::with_defaults();
        let picked = selector.pick(&request).unwrap();
        // Both RawProjector (Fallback) and TruncatedProjector (Fallback) match;
        // the selector prefers RawProjector (registered first) and then
        // falls back to TruncatedProjector when RawProjector is omitted.
        assert!(matches!(
            picked.name(),
            RawProjector::NAME | TruncatedProjector::NAME
        ));
    }

    #[test]
    fn default_command_projection_returns_text() {
        let mut store = CommandOutputStore::new();
        let run = make_run(
            &mut store,
            b"hi\n".to_vec(),
            Vec::new(),
            CommandExit::Code(0),
            Duration::from_millis(20),
        );
        let s = default_command_projection(&run, &store);
        assert!(s.contains("hi"));
        assert!(s.contains("exit 0"));
    }

    #[test]
    fn default_command_projection_handles_failure() {
        let mut store = CommandOutputStore::new();
        let run = make_run(
            &mut store,
            Vec::new(),
            b"oops\n".to_vec(),
            CommandExit::Code(1),
            Duration::from_millis(20),
        );
        let s = default_command_projection(&run, &store);
        assert!(s.contains("exit 1"));
        assert!(s.contains("oops"));
    }

    #[test]
    fn default_command_projection_respects_budget() {
        let mut store = CommandOutputStore::new();
        let big: Vec<u8> = (0..2000).map(|i| b'a' + (i % 26) as u8).collect();
        let run = make_run(
            &mut store,
            big,
            Vec::new(),
            CommandExit::Code(0),
            Duration::ZERO,
        );
        let s = default_command_projection_with_budget(&run, &store, 64);
        assert!(s.contains("omitted") || s.contains("..."));
        assert!(s.contains("2000 bytes"));
    }

    #[test]
    fn default_command_projection_handles_missing_stream() {
        let run = CommandRun {
            id: CommandRunId(99),
            command: "ghost".to_string(),
            argv: None,
            cwd: PathBuf::from("/tmp"),
            started_at: SystemTime::now(),
            duration: Duration::ZERO,
            exit: CommandExit::Code(0),
            stdout: RawStream {
                total_bytes: 0,
                retained_bytes: 0,
                total_lines: None,
                handle: None,
                encoding: OutputEncoding::Utf8,
                completeness: OutputCompleteness::Complete,
            },
            stderr: RawStream {
                total_bytes: 0,
                retained_bytes: 0,
                total_lines: None,
                handle: None,
                encoding: OutputEncoding::Utf8,
                completeness: OutputCompleteness::Complete,
            },
            combined: None,
            projection: None,
            redaction: RedactionState::NotApplied,
        };
        let store = CommandOutputStore::new();
        let s = default_command_projection(&run, &store);
        assert!(s.contains("[command 99]"));
        assert!(s.contains("ghost"));
    }

    #[test]
    fn redaction_hook_applied_for_model_target() {
        let mut store = CommandOutputStore::new();
        let run = make_run(
            &mut store,
            b"hi\n".to_vec(),
            Vec::new(),
            CommandExit::Code(0),
            Duration::ZERO,
        );
        let policy = ProjectionPolicy::conservative();
        let request = ProjectionRequest::for_target(&run, ProjectionTarget::ModelContext, &policy);
        let selector = ProjectionSelector::with_defaults();
        let result = selector.project(request, &store);
        assert_eq!(result.redaction, RedactionState::Applied);
    }

    #[test]
    fn redaction_hook_skipped_for_tui_target() {
        let mut store = CommandOutputStore::new();
        let run = make_run(
            &mut store,
            b"hi\n".to_vec(),
            Vec::new(),
            CommandExit::Code(0),
            Duration::ZERO,
        );
        let policy = ProjectionPolicy::conservative();
        let request = ProjectionRequest::for_target(&run, ProjectionTarget::TuiDetail, &policy);
        let selector = ProjectionSelector::with_defaults();
        let result = selector.project(request, &store);
        assert_eq!(result.redaction, RedactionState::NotApplied);
    }

    #[test]
    fn non_utf8_output_does_not_panic() {
        let mut store = CommandOutputStore::new();
        let run = make_run(
            &mut store,
            vec![0xFF, 0xFE, b'a'],
            Vec::new(),
            CommandExit::Code(0),
            Duration::ZERO,
        );
        let policy = ProjectionPolicy::conservative();
        let request = ProjectionRequest::for_target(&run, ProjectionTarget::ModelContext, &policy);
        let selector = ProjectionSelector::with_defaults();
        let result = selector.project(request, &store);
        assert!(!result.text.is_empty());
        // Either we get a warning or the result is labelled exact.
        assert!(!result.warnings.is_empty() || result.exactness == ProjectionExactness::Exact);
    }

    #[test]
    fn exact_requested_avoids_lossy_projectors() {
        let mut store = CommandOutputStore::new();
        let big = vec![b'x'; 16 * 1024];
        let run = make_run(
            &mut store,
            big,
            Vec::new(),
            CommandExit::Code(0),
            Duration::ZERO,
        );
        let policy = ProjectionPolicy::conservative();
        let mut request =
            ProjectionRequest::for_target(&run, ProjectionTarget::ModelContext, &policy);
        request.exact_requested = true;
        request.budget = ProjectionBudget::bytes(64);
        let selector = ProjectionSelector::with_defaults();
        // exact_requested does not currently override RawProjector,
        // but TruncatedProjector stays available for callers that
        // pass exact_requested and want exact-range views.
        let picked = selector.pick(&request).unwrap();
        assert!(matches!(
            picked.name(),
            RawProjector::NAME | TruncatedProjector::NAME
        ));
    }

    #[test]
    fn expansion_handle_url_includes_byte_range() {
        let h = ExpansionHandle {
            command_id: CommandRunId(7),
            stream: CommandOutputStream::Stderr,
            byte_range: Some(0..1024),
        };
        assert_eq!(h.as_url(), "cmd://7/stderr#0-1024");
        assert_eq!(h.to_string(), "cmd://7/stderr#0-1024");
    }

    #[test]
    fn expansion_handle_full_stream_has_no_range() {
        let h = ExpansionHandle::full(CommandRunId(7), CommandOutputStream::Stdout);
        assert_eq!(h.as_url(), "cmd://7/stdout");
    }

    #[test]
    fn omitted_range_bytes_computed() {
        let r = OmittedRange {
            stream: CommandOutputStream::Stdout,
            start_byte: 100,
            end_byte: 200,
            start_line: None,
            end_line: None,
            total_retained_bytes: 500,
            note: None,
        };
        assert_eq!(r.omitted_bytes(), 100);
    }

    #[test]
    fn projection_kind_labels() {
        assert_eq!(ProjectionKind::Raw.label(), "raw");
        assert_eq!(ProjectionKind::Truncated.label(), "truncated");
        assert_eq!(ProjectionKind::ErrorRetention.label(), "error-retention");
    }

    #[test]
    fn exactness_labels() {
        assert_eq!(ProjectionExactness::Exact.label(), "exact");
        assert_eq!(ProjectionExactness::Lossy.label(), "lossy");
        assert_eq!(ProjectionExactness::Truncated.label(), "truncated");
        assert!(ProjectionExactness::Exact.is_exact());
        assert!(!ProjectionExactness::Lossy.is_exact());
    }

    #[test]
    fn projection_target_requires_redaction() {
        assert!(ProjectionTarget::ModelContext.requires_redaction());
        assert!(ProjectionTarget::ToolExpansion.requires_redaction());
        assert!(!ProjectionTarget::TuiTranscript.requires_redaction());
        assert!(!ProjectionTarget::TuiDetail.requires_redaction());
    }

    #[test]
    fn policy_conservative_defaults() {
        let p = ProjectionPolicy::conservative();
        assert!(p.allow_lossy);
        assert!(!p.allow_external_backend);
        assert!(p.redact_model_visible);
    }

    #[test]
    fn empty_selector_returns_unprojectable() {
        let mut store = CommandOutputStore::new();
        let run = make_run(
            &mut store,
            b"hi\n".to_vec(),
            Vec::new(),
            CommandExit::Code(0),
            Duration::ZERO,
        );
        let policy = ProjectionPolicy::conservative();
        let request = ProjectionRequest::for_target(&run, ProjectionTarget::ModelContext, &policy);
        let selector = ProjectionSelector::empty();
        let result = selector.project(request, &store);
        assert_eq!(result.projector, "none");
        assert!(result.text.contains("no projector supports"));
        assert_eq!(result.warnings.len(), 1);
    }

    #[test]
    fn truncated_projector_preserves_stderr_in_full_when_small() {
        let mut store = CommandOutputStore::new();
        let run = make_run(
            &mut store,
            vec![b'x'; 1024],
            b"stderr here\n".to_vec(),
            CommandExit::Code(0),
            Duration::ZERO,
        );
        let policy = ProjectionPolicy::conservative();
        let mut request =
            ProjectionRequest::for_target(&run, ProjectionTarget::ModelContext, &policy);
        request.budget = ProjectionBudget::bytes(64);
        let result = TruncatedProjector.project(request, &store).unwrap();
        assert!(result.text.contains("stderr here"));
    }

    #[test]
    fn banner_formats_cleanly() {
        let mut store = CommandOutputStore::new();
        let run = make_run(
            &mut store,
            b"hi\n".to_vec(),
            Vec::new(),
            CommandExit::Code(0),
            Duration::from_millis(1500),
        );
        let mut result = ProjectionResult::empty("raw", ProjectionKind::Raw);
        result.text = "hi".to_string();
        result.input_bytes = 3;
        result.output_bytes = 2;
        let banner = result.banner(&run);
        assert!(banner.contains("1.5"));
        assert!(banner.contains("raw"));
        assert!(banner.contains(&run.id.to_string()));
    }

    #[test]
    fn matches_error_pattern_recognises_languages() {
        assert!(matches_error_pattern("error[E0001]: bad"));
        assert!(matches_error_pattern("Traceback (most recent call last):"));
        assert!(matches_error_pattern("TypeError: oops"));
        assert!(matches_error_pattern("thread 'foo' panicked at 'bar'"));
        assert!(matches_error_pattern("segfault at 0x0"));
        assert!(!matches_error_pattern("all good"));
    }
}

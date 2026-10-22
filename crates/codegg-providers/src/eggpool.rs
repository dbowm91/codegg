//! A small, bounded probe for Eggpool's OpenAI-compatible `/models` endpoint.
//!
//! This module is intentionally independent of connection storage and protocol
//! types.  It is the providers-side seam used by those layers to validate an
//! endpoint and obtain a small, deterministic model catalog.
//!
//! The probe machinery is protocol-generic (strict OpenAI-compatible
//! `/models` validation with redirect/body/model-count bounds); only the
//! default port and preset normalization stay Eggpool-specific. The
//! `Compatible*` aliases below are the provider-neutral names new code must
//! use. Only [`normalize_eggpool_base_url`] keeps Eggpool semantics (it
//! applies the preset default port); arbitrary compatible endpoints must use
//! their own validation without a preset port.

use codegg_config::schema::{
    ReasoningCapabilityRecord, ReasoningCapabilityStatus, MAX_REASONING_CAPABILITY_LABELS,
    MAX_REASONING_CAPABILITY_LABEL_BYTES,
};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::btree_map::Entry;
use std::collections::BTreeMap;
use std::fmt;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::sync::RwLock;
use std::time::Duration;
use tokio::sync::Notify;
use url::Url;

pub const EGGPOOL_DEFAULT_PORT: u16 = 11_300;
const DEFAULT_RESPONSE_BYTE_LIMIT: usize = 1024 * 1024;
const DEFAULT_MODEL_COUNT_LIMIT: usize = 256;
const DEFAULT_MODEL_STRING_LIMIT: usize = 256;
const DEFAULT_CONNECT_TIMEOUT: Duration = Duration::from_secs(5);
const DEFAULT_REQUEST_TIMEOUT: Duration = Duration::from_secs(10);
const DEFAULT_OVERALL_TIMEOUT: Duration = Duration::from_secs(15);

/// Provider-neutral aliases for the strict compatible `/models` probe.
/// New code must use these names; the `Eggpool*` names remain for
/// compatibility. `CompatibleModelsProbe` uses generic endpoint semantics
/// (explicit http/https, preserved port/path, never injects `:11300`);
/// `EggpoolProbe` retains the Eggpool preset (host shorthand + default port).
pub use CompatibleProbe as CompatibleModelsProbe;
pub use EggpoolApiKey as CompatibleApiKey;
pub use EggpoolCancellationToken as CompatibleCancellationToken;
pub use EggpoolModelSummary as CompatibleModelSummary;
pub use EggpoolProbeError as CompatibleProbeError;
pub use EggpoolProbeOptions as CompatibleProbeOptions;
pub use EggpoolProbeReasonCode as CompatibleProbeReasonCode;
pub use EggpoolProbeSummary as CompatibleProbeSummary;

/// Stable, redacted categories for probe failures.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EggpoolProbeReasonCode {
    Auth,
    Unreachable,
    Timeout,
    Tls,
    InvalidJson,
    Unsupported,
    Redirect,
    Empty,
    Oversized,
    Cancelled,
    InvalidInput,
}

impl EggpoolProbeReasonCode {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Auth => "auth",
            Self::Unreachable => "unreachable",
            Self::Timeout => "timeout",
            Self::Tls => "tls",
            Self::InvalidJson => "invalid_json",
            Self::Unsupported => "unsupported",
            Self::Redirect => "redirect_disallowed",
            Self::Empty => "empty",
            Self::Oversized => "oversized",
            Self::Cancelled => "cancelled",
            Self::InvalidInput => "invalid_input",
        }
    }
}

impl fmt::Display for EggpoolProbeReasonCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// A redacted probe error.  It deliberately carries no transport or serde
/// source error because those can contain a URL, response text, or headers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EggpoolProbeError {
    reason: EggpoolProbeReasonCode,
}

impl EggpoolProbeError {
    const fn new(reason: EggpoolProbeReasonCode) -> Self {
        Self { reason }
    }

    pub const fn reason_code(self) -> EggpoolProbeReasonCode {
        self.reason
    }
}

impl fmt::Display for EggpoolProbeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "eggpool probe failed: {}", self.reason)
    }
}

impl std::error::Error for EggpoolProbeError {}

/// API-key input kept behind a type whose debug and display forms are safe.
pub struct EggpoolApiKey(String);

impl EggpoolApiKey {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }
}

impl From<String> for EggpoolApiKey {
    fn from(value: String) -> Self {
        Self::new(value)
    }
}

impl From<&str> for EggpoolApiKey {
    fn from(value: &str) -> Self {
        Self::new(value)
    }
}

impl fmt::Debug for EggpoolApiKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("EggpoolApiKey(<redacted>)")
    }
}

impl fmt::Display for EggpoolApiKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("<redacted>")
    }
}

/// Cancellation handle for an in-flight probe.
#[derive(Clone, Debug, Default)]
pub struct EggpoolCancellationToken {
    cancelled: Arc<AtomicBool>,
    notify: Arc<Notify>,
}

impl EggpoolCancellationToken {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::Release);
        self.notify.notify_waiters();
    }

    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::Acquire)
    }

    async fn cancelled(&self) {
        if self.is_cancelled() {
            return;
        }
        let notified = self.notify.notified();
        if self.is_cancelled() {
            return;
        }
        notified.await;
    }
}

/// Resource bounds and timeout controls for one probe.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EggpoolProbeOptions {
    pub connect_timeout: Duration,
    pub request_timeout: Duration,
    pub overall_timeout: Duration,
    pub response_byte_limit: usize,
    pub model_count_limit: usize,
    pub model_string_limit: usize,
}

impl Default for EggpoolProbeOptions {
    fn default() -> Self {
        Self {
            connect_timeout: DEFAULT_CONNECT_TIMEOUT,
            request_timeout: DEFAULT_REQUEST_TIMEOUT,
            overall_timeout: DEFAULT_OVERALL_TIMEOUT,
            response_byte_limit: DEFAULT_RESPONSE_BYTE_LIMIT,
            model_count_limit: DEFAULT_MODEL_COUNT_LIMIT,
            model_string_limit: DEFAULT_MODEL_STRING_LIMIT,
        }
    }
}

/// A bounded, stable projection of one discovered model.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EggpoolModelSummary {
    pub id: String,
    pub name: String,
}

/// Successful probe output.  The digest is computed from the sorted bounded
/// summaries, so response order and irrelevant provider fields do not matter.
///
/// The retained thinking-capability overlay rides **alongside** the models,
/// never inside them: the digest hashes `id` and `name` only, and that is
/// load-bearing, because the digest is the revision-scoped `catalog_revision`
/// that stored last-used-model preferences are keyed against.
#[derive(Debug, Clone, PartialEq)]
pub struct EggpoolProbeSummary {
    pub models: Vec<EggpoolModelSummary>,
    pub digest: String,
    /// The thinking-capability overlays the response published.
    ///
    /// Empty when the provider published no contract. The overlay is
    /// **not** part of `digest`: it has its own revision.
    pub capability_overlay: EggpoolCapabilityOverlay,
}

// ---------------------------------------------------------------------------
// The discovery overlay (ADR-0045, milestone 005).
//
// A provider may publish a thinking-capability contract alongside a model in
// its catalog response. The contract is **untrusted input**: it may only
// **narrow** the compiled adapter baseline, it is parsed into a bounded,
// typed shape before any use, and it is held apart from the model summary so
// the model catalog revision keeps its current meaning and its current
// inputs.
//
// The pinned upstream publishes **no** capability block on `/v1/models` at
// all — the one capability shape it exposes at runtime rides an
// integrations-profile route CodeGG does not call — so the machinery below
// is forward-compatible and inert at this baseline. It is written against
// both shapes a provider could publish: the flat `thinking` capability and
// the documented `control_contract.accepted_efforts` shape.
// ---------------------------------------------------------------------------

/// The largest thinking-budget bound an overlay may retain, in tokens.
///
/// A bound above this is not a bound, it is untrusted content that would
/// inflate the option surface. The bound is discarded rather than clamped:
/// a clamped bound would be a number the provider never published.
pub const MAX_OVERLAY_THINKING_BUDGET_TOKENS: usize = 16_777_216;

/// The locations of a published thinking-capability block inside one catalog
/// entry.
///
/// The documented envelope nests the capability under
/// `eggpool.capabilities.thinking`; the flat runtime capability sits at
/// `capabilities.thinking`, or directly on the entry as `thinking`. All
/// three are read, because none of them is published today.
const OVERLAY_CAPABILITY_PATHS: [&str; 3] = [
    "/eggpool/capabilities/thinking",
    "/capabilities/thinking",
    "/thinking",
];

/// The keys a published accepted-effort ladder may use.
///
/// The documented contract shape spells it `accepted_efforts`; the flat
/// runtime capability spells it `supported_efforts`.
const OVERLAY_LABEL_KEYS: [&str; 2] = ["accepted_efforts", "supported_efforts"];

/// A bounded, typed thinking-capability overlay retained for one model.
///
/// The overlay carries the **narrowing** record — the only part that ever
/// reaches the capability resolution — plus the provider's always-reasons
/// claim, which is retained for reporting and deliberately **not** part of
/// the record: the compiled baseline owns the always-reasons property
/// (ADR-0045 §1), and a provider's advertisement is not an authority over
/// its own model's behaviour.
#[derive(Debug, Clone, PartialEq)]
pub struct EggpoolThinkingOverlay {
    record: ReasoningCapabilityRecord,
    always_reasons_claim: Option<bool>,
}

impl EggpoolThinkingOverlay {
    /// The narrowing record.
    ///
    /// This is what the capability resolution consumes. It declares only
    /// what the provider published, and every dimension it declares is a
    /// narrowing of the compiled baseline.
    pub fn record(&self) -> &ReasoningCapabilityRecord {
        &self.record
    }

    /// The provider's always-reasons claim, if it published one.
    ///
    /// Retained for reporting only. It is never part of [`Self::record`],
    /// so the overlay can never change the always-reasons property.
    pub fn always_reasons_claim(&self) -> Option<bool> {
        self.always_reasons_claim
    }
}

/// The thinking-capability overlays retained from one catalog response.
///
/// The aggregate is immutable and is replaced as a whole, so a reader
/// observes either the whole previous overlay or the whole next one.
#[derive(Debug, Clone, PartialEq)]
pub struct EggpoolCapabilityOverlay {
    /// The catalog revision this overlay belongs to.
    ///
    /// Empty until a caller binds it: the daemon computes the catalog
    /// revision *after* routing qualification, so the probe cannot know it.
    /// Holding it here makes the overlay **revision-scoped**, like the
    /// catalog rows it belongs to, so a reader can tell an overlay that
    /// belongs to the current catalog from one that does not.
    catalog_revision: String,
    /// The capability revision: a digest over the overlay content alone.
    ///
    /// It is deliberately **separate** from the model catalog revision. The
    /// catalog revision hashes `id` and `name` only, so folding the
    /// capability into it would invalidate every stored last-used-model
    /// preference and split the revision-scoped model rows.
    capability_revision: String,
    /// The per-model overlays, keyed by model id.
    overlays: BTreeMap<String, EggpoolThinkingOverlay>,
}

impl EggpoolCapabilityOverlay {
    /// Build the aggregate from the per-model overlays a response carried.
    ///
    /// The capability revision is computed over the overlay content alone.
    pub fn new(overlays: BTreeMap<String, EggpoolThinkingOverlay>) -> Self {
        let capability_revision = digest_overlays(&overlays);
        Self {
            catalog_revision: String::new(),
            capability_revision,
            overlays,
        }
    }

    /// The catalog revision this overlay belongs to.
    pub fn catalog_revision(&self) -> &str {
        &self.catalog_revision
    }

    /// The capability revision: a digest over the overlay content alone.
    pub fn capability_revision(&self) -> &str {
        &self.capability_revision
    }

    /// Whether the provider published no thinking-capability contract.
    pub fn is_empty(&self) -> bool {
        self.overlays.is_empty()
    }

    /// The overlay for one model, if the provider published one for it.
    pub fn for_model(&self, model_id: &str) -> Option<&EggpoolThinkingOverlay> {
        self.overlays.get(model_id)
    }

    /// Every retained overlay, keyed by model id.
    pub fn overlays(&self) -> &BTreeMap<String, EggpoolThinkingOverlay> {
        &self.overlays
    }

    /// Bind this overlay to a catalog revision.
    ///
    /// Binding it is what makes it revision-scoped: a reader that resolved
    /// a different catalog revision treats the overlay as absent, so it
    /// falls back to the compiled baseline rather than narrowing with
    /// another revision's contract.
    pub fn with_catalog_revision(mut self, revision: &str) -> Self {
        self.catalog_revision = revision.to_owned();
        self
    }
}

impl Default for EggpoolCapabilityOverlay {
    fn default() -> Self {
        Self::new(BTreeMap::new())
    }
}

/// The holder for one connection's capability overlay.
///
/// The overlay is **replaced atomically**: the whole value is swapped under
/// a write lock, and a reader clones the current `Arc` and releases the
/// lock. A reader therefore observes either the whole previous overlay or
/// the whole next one, never a half-replaced one — the same all-or-nothing
/// property the catalog rows get from the refresh transaction. The lock is
/// never held across an await; the critical section is one `Arc` clone.
#[derive(Debug)]
pub struct EggpoolCapabilityOverlaySlot {
    current: RwLock<Arc<EggpoolCapabilityOverlay>>,
}

impl EggpoolCapabilityOverlaySlot {
    /// A slot holding no overlay.
    pub fn new() -> Self {
        Self {
            current: RwLock::new(Arc::new(EggpoolCapabilityOverlay::default())),
        }
    }

    /// Replace the overlay as a whole.
    ///
    /// The swap is the only write path: there is no way to mutate an
    /// installed overlay in place, so a reader can never observe a partial
    /// update.
    pub fn replace(&self, overlay: EggpoolCapabilityOverlay) {
        let mut current = self
            .current
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        *current = Arc::new(overlay);
    }

    /// The current overlay.
    ///
    /// The returned `Arc` is a whole snapshot: the request path reads it
    /// and never mutates it.
    pub fn load(&self) -> Arc<EggpoolCapabilityOverlay> {
        let current = self
            .current
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        Arc::clone(&current)
    }
}

impl Default for EggpoolCapabilityOverlaySlot {
    fn default() -> Self {
        Self::new()
    }
}

/// Compute the deterministic capability revision for a set of retained
/// overlays.
///
/// The digest covers the overlay content alone — never the model summary —
/// so a capability change is detectable without disturbing the model
/// catalog revision. The overlays are visited in model-id order and each
/// ladder is sorted, so the response's ordering does not move the revision.
pub fn digest_overlays(overlays: &BTreeMap<String, EggpoolThinkingOverlay>) -> String {
    let mut hasher = Sha256::new();
    for (model_id, overlay) in overlays {
        hash_string(&mut hasher, model_id);
        let record = &overlay.record;
        hash_string(&mut hasher, status_token(record.effort_status));
        match &record.accepted_effort_labels {
            Some(labels) => {
                hash_string(&mut hasher, "labels");
                for label in labels {
                    hash_string(&mut hasher, label);
                }
            }
            None => hash_string(&mut hasher, "no-labels"),
        }
        hash_string(&mut hasher, status_token(record.disable_status));
        hash_string(&mut hasher, status_token(record.budget_status));
        match record.min_thinking_budget {
            Some(value) => hash_string(&mut hasher, &value.to_string()),
            None => hash_string(&mut hasher, "no-min"),
        }
        match record.max_thinking_budget {
            Some(value) => hash_string(&mut hasher, &value.to_string()),
            None => hash_string(&mut hasher, "no-max"),
        }
    }
    hex::encode(hasher.finalize())
}

/// The stable token one capability status hashes as.
fn status_token(status: ReasoningCapabilityStatus) -> &'static str {
    match status {
        ReasoningCapabilityStatus::Supported => "supported",
        ReasoningCapabilityStatus::Unsupported => "unsupported",
        ReasoningCapabilityStatus::Unknown => "unknown",
        ReasoningCapabilityStatus::Mixed => "mixed",
        ReasoningCapabilityStatus::Conflicting => "conflicting",
    }
}

/// What a published effort ladder turned out to be.
enum OverlayLabels {
    /// No ladder was published.
    Absent,
    /// A bounded, deduplicated ladder.
    Present(Vec<String>),
    /// A ladder was published but it is malformed or oversized.
    Malformed,
}

/// What a published thinking-budget bound turned out to be.
enum OverlayBound {
    /// No bound was published.
    Absent,
    /// A bound was published but it is malformed or oversized.
    Malformed,
    /// A usable bound.
    Present(usize),
}

/// Parse the published thinking-capability contract of one catalog entry.
///
/// Returns `None` when the entry carries no usable contract, and a record
/// whose dimensions are `Unknown` where the published content is malformed,
/// oversized, or of the wrong type. The parse never fails: a malformed
/// contract must not break catalog discovery.
fn parse_thinking_overlay(entry: &Value) -> Option<EggpoolThinkingOverlay> {
    let thinking = OVERLAY_CAPABILITY_PATHS
        .iter()
        .find_map(|path| entry.pointer(path))
        .filter(|value| value.is_object())?;
    let contract = thinking
        .get("control_contract")
        .filter(|value| value.is_object());

    // The always-reasons claim is retained for reporting only; it never
    // reaches the narrowing record.
    let always_reasons_claim = overlay_always_reasons(thinking, contract);

    // The effort dimension: the published ladder and the published status.
    //
    // One rule set governs every dimension: malformed content takes the
    // dimension with it, a contradiction degrades it, and a published
    // ladder is itself a declaration of the dimension it belongs to.
    let labels = match overlay_labels_field(thinking, contract) {
        None => OverlayLabels::Absent,
        Some(value) => parse_overlay_labels(value),
    };
    let published_effort_status =
        parse_overlay_status(overlay_status_field(thinking, contract, "effort").as_deref());
    let (effort_status, accepted_effort_labels) = match labels {
        // Malformed content takes the dimension with it.
        OverlayLabels::Malformed => (ReasoningCapabilityStatus::Unknown, None),
        OverlayLabels::Absent => match published_effort_status {
            // A supported effort dimension must carry a ladder. Without one
            // the status would be a claim the provider did not publish.
            ReasoningCapabilityStatus::Supported => (ReasoningCapabilityStatus::Unknown, None),
            status => (status, None),
        },
        OverlayLabels::Present(labels) => match published_effort_status {
            // A ladder beside a refusal contradicts the refusal.
            ReasoningCapabilityStatus::Unsupported => (ReasoningCapabilityStatus::Unknown, None),
            // A published ladder is itself a declaration that the effort
            // dimension is supported.
            ReasoningCapabilityStatus::Unknown => {
                (ReasoningCapabilityStatus::Supported, Some(labels))
            }
            status => (status, Some(labels)),
        },
    };

    // The disable dimension: may reasoning be turned off at all?
    let disable_status =
        parse_overlay_status(overlay_status_field(thinking, contract, "toggle").as_deref());

    // The budget dimension: is a fixed token budget accepted, and within
    // which bounds?
    let published_budget_status =
        parse_overlay_status(overlay_status_field(thinking, contract, "budget").as_deref());
    let published_bounds = match (
        overlay_bound_field(thinking, contract, "budget_tokens_min"),
        overlay_bound_field(thinking, contract, "budget_tokens_max"),
    ) {
        // Malformed content takes the dimension with it.
        (OverlayBound::Malformed, _) | (_, OverlayBound::Malformed) => None,
        // A range whose minimum exceeds its maximum is not a range.
        (OverlayBound::Present(min), OverlayBound::Present(max)) if min > max => None,
        (OverlayBound::Present(min), OverlayBound::Present(max)) => Some((Some(min), Some(max))),
        (OverlayBound::Present(min), OverlayBound::Absent) => Some((Some(min), None)),
        (OverlayBound::Absent, OverlayBound::Present(max)) => Some((None, Some(max))),
        (OverlayBound::Absent, OverlayBound::Absent) => Some((None, None)),
    };
    let (budget_status, min_thinking_budget, max_thinking_budget) = match published_bounds {
        None => (ReasoningCapabilityStatus::Unknown, None, None),
        Some((min, max)) => match published_budget_status {
            // A range beside a refusal contradicts the refusal.
            ReasoningCapabilityStatus::Unsupported if min.is_some() || max.is_some() => {
                (ReasoningCapabilityStatus::Unknown, None, None)
            }
            // A published range is itself a declaration that a fixed
            // budget is accepted.
            ReasoningCapabilityStatus::Unknown if min.is_some() || max.is_some() => {
                (ReasoningCapabilityStatus::Supported, min, max)
            }
            status => (status, min, max),
        },
    };

    let record = ReasoningCapabilityRecord {
        effort_status,
        accepted_effort_labels,
        disable_status,
        budget_status,
        min_thinking_budget,
        max_thinking_budget,
        // The always-reasons property belongs to the compiled baseline.
        always_reasons_status: ReasoningCapabilityStatus::Unknown,
        always_reasons: None,
    };

    if !record_declares_anything(&record) {
        // Nothing usable was published: there is no overlay.
        return None;
    }

    Some(EggpoolThinkingOverlay {
        record,
        always_reasons_claim,
    })
}

/// Whether a record declares no dimension at all.
fn record_declares_anything(record: &ReasoningCapabilityRecord) -> bool {
    record.effort_status != ReasoningCapabilityStatus::Unknown
        || record.accepted_effort_labels.is_some()
        || record.disable_status != ReasoningCapabilityStatus::Unknown
        || record.budget_status != ReasoningCapabilityStatus::Unknown
        || record.min_thinking_budget.is_some()
        || record.max_thinking_budget.is_some()
}

/// Read one published status string, preferring the documented
/// `control_contract` spelling over the flat one.
fn overlay_status_field(thinking: &Value, contract: Option<&Value>, key: &str) -> Option<String> {
    contract
        .and_then(|value| value.get(key))
        .and_then(Value::as_str)
        .or_else(|| thinking.get(key).and_then(Value::as_str))
        .map(str::to_owned)
}

/// Read the published accepted-effort ladder, whichever key carries it.
fn overlay_labels_field<'a>(thinking: &'a Value, contract: Option<&'a Value>) -> Option<&'a Value> {
    for source in [contract, Some(thinking)].into_iter().flatten() {
        for key in OVERLAY_LABEL_KEYS {
            if let Some(value) = source.get(key) {
                return Some(value);
            }
        }
    }
    None
}

/// Read one published thinking-budget bound.
fn overlay_bound_field(thinking: &Value, contract: Option<&Value>, key: &str) -> OverlayBound {
    let raw = contract
        .and_then(|value| value.get(key))
        .or_else(|| thinking.get(key));
    match raw {
        None => OverlayBound::Absent,
        Some(value) => match value.as_u64() {
            Some(tokens) => match usize::try_from(tokens) {
                Ok(tokens) if tokens > 0 && tokens <= MAX_OVERLAY_THINKING_BUDGET_TOKENS => {
                    OverlayBound::Present(tokens)
                }
                _ => OverlayBound::Malformed,
            },
            None => OverlayBound::Malformed,
        },
    }
}

/// Read the provider's always-reasons claim.
fn overlay_always_reasons(thinking: &Value, contract: Option<&Value>) -> Option<bool> {
    contract
        .and_then(|value| value.get("always_reasons"))
        .or_else(|| thinking.get("always_reasons"))
        .and_then(Value::as_bool)
}

/// Parse one published capability status.
///
/// The provider's vocabulary is the kernel's: `supported`, `unsupported`,
/// `unknown`, `mixed`. An unrecognised spelling is not a status; it
/// degrades to `Unknown` rather than being guessed at.
fn parse_overlay_status(value: Option<&str>) -> ReasoningCapabilityStatus {
    match value
        .map(|value| value.trim().to_ascii_lowercase())
        .as_deref()
    {
        Some("supported") => ReasoningCapabilityStatus::Supported,
        Some("unsupported") => ReasoningCapabilityStatus::Unsupported,
        Some("mixed") => ReasoningCapabilityStatus::Mixed,
        _ => ReasoningCapabilityStatus::Unknown,
    }
}

/// Parse a published accepted-effort ladder into a bounded, deduplicated set.
///
/// The ladder is **deduplicated** (a repeated label is the same label) and
/// **bounded**: at most [`MAX_REASONING_CAPABILITY_LABELS`] labels, each at
/// most [`MAX_REASONING_CAPABILITY_LABEL_BYTES`] bytes. Content that
/// violates a bound is discarded whole — the dimension degrades to
/// `Unknown` — rather than truncated, because a truncated ladder would
/// present a subset as if it were the published one.
fn parse_overlay_labels(value: &Value) -> OverlayLabels {
    let Some(array) = value.as_array() else {
        return OverlayLabels::Malformed;
    };
    if array.is_empty() {
        // An empty ladder publishes no label, so it declares no ladder at
        // all. It is not "this model accepts no effort", which would be a
        // claim the provider never made.
        return OverlayLabels::Absent;
    }
    if array.len() > MAX_REASONING_CAPABILITY_LABELS {
        return OverlayLabels::Malformed;
    }
    let mut labels: Vec<String> = Vec::with_capacity(array.len());
    for entry in array {
        let Some(label) = entry.as_str() else {
            return OverlayLabels::Malformed;
        };
        if label.is_empty()
            || label.trim() != label
            || label.len() > MAX_REASONING_CAPABILITY_LABEL_BYTES
        {
            return OverlayLabels::Malformed;
        }
        if !labels.iter().any(|existing| existing == label) {
            labels.push(label.to_owned());
        }
    }
    // Sorted, so the ladder's published order does not move the capability
    // revision.
    labels.sort();
    OverlayLabels::Present(labels)
}

/// Normalize an Eggpool/OpenAI-compatible base URL.
///
/// Host-only input is accepted as HTTP and uses Eggpool's default port.  A
/// supplied scheme, port, and path are preserved after removing trailing
/// slashes.  Userinfo, query strings, fragments, control characters, and path
/// traversal are rejected before any network request is possible.
pub fn normalize_eggpool_base_url(input: &str) -> Result<String, EggpoolProbeError> {
    let input = input.trim();
    if input.is_empty() || input.chars().any(char::is_control) {
        return Err(EggpoolProbeError::new(EggpoolProbeReasonCode::InvalidInput));
    }

    let candidate = if input.contains("://") {
        input.to_owned()
    } else {
        format!("http://{input}")
    };
    if has_path_traversal(&candidate) {
        return Err(EggpoolProbeError::new(EggpoolProbeReasonCode::InvalidInput));
    }
    let mut url = Url::parse(&candidate)
        .map_err(|_| EggpoolProbeError::new(EggpoolProbeReasonCode::InvalidInput))?;

    if !matches!(url.scheme(), "http" | "https")
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
        || url.port() == Some(0)
    {
        return Err(EggpoolProbeError::new(EggpoolProbeReasonCode::InvalidInput));
    }

    if url
        .path_segments()
        .map(|mut segments| segments.any(|segment| segment == ".."))
        .unwrap_or(false)
    {
        return Err(EggpoolProbeError::new(EggpoolProbeReasonCode::InvalidInput));
    }

    if url.port().is_none() {
        url.set_port(Some(EGGPOOL_DEFAULT_PORT))
            .map_err(|_| EggpoolProbeError::new(EggpoolProbeReasonCode::InvalidInput))?;
    }

    let path = url.path().trim_end_matches('/').to_owned();
    url.set_path(&path);
    Ok(url.as_str().trim_end_matches('/').to_owned())
}

/// Normalize a generic OpenAI-compatible base URL.
///
/// Provider-neutral semantics for arbitrary compatible upstreams:
/// requires an explicit `http://` or `https://` endpoint, preserves a
/// supplied port and path after removing trailing slashes, and never
/// injects the Eggpool preset default port. Userinfo, query strings,
/// fragments, control characters, and path traversal are rejected before
/// any network request is possible.
pub fn normalize_compatible_base_url(input: &str) -> Result<String, EggpoolProbeError> {
    let input = input.trim();
    if input.is_empty() || input.chars().any(char::is_control) {
        return Err(EggpoolProbeError::new(EggpoolProbeReasonCode::InvalidInput));
    }
    if has_path_traversal(input) {
        return Err(EggpoolProbeError::new(EggpoolProbeReasonCode::InvalidInput));
    }
    let mut url = Url::parse(input)
        .map_err(|_| EggpoolProbeError::new(EggpoolProbeReasonCode::InvalidInput))?;

    if !matches!(url.scheme(), "http" | "https")
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
        || url.port() == Some(0)
    {
        return Err(EggpoolProbeError::new(EggpoolProbeReasonCode::InvalidInput));
    }

    if url
        .path_segments()
        .map(|mut segments| segments.any(|segment| segment == ".."))
        .unwrap_or(false)
    {
        return Err(EggpoolProbeError::new(EggpoolProbeReasonCode::InvalidInput));
    }

    // Never inject a preset port: a missing port stays missing so generic
    // upstreams keep their origin exactly as configured.
    let path = url.path().trim_end_matches('/').to_owned();
    url.set_path(&path);
    Ok(url.as_str().trim_end_matches('/').to_owned())
}

/// Shared bounded `/models` response parser for strict and best-effort
/// compatible discovery.
///
/// Enforces the single response-byte, model-count, and string-length bounds
/// from [`EggpoolProbeOptions`], normalizes the supported OpenAI-compatible
/// list shape (`{"data":[{"id","name"?}]`), and handles duplicate IDs
/// deterministically (one entry per ID, lexicographically smallest name
/// wins). Returns the bounded normalized summaries; callers apply their own
/// failure policy (strict fails, best-effort falls back to seeds).
pub fn parse_compatible_models_response(
    body: &[u8],
    options: &EggpoolProbeOptions,
) -> Result<Vec<EggpoolModelSummary>, EggpoolProbeError> {
    let summary = parse_summary(body, options)?;
    Ok(summary.models)
}

/// Parse a bounded `/models` response into its models **and** the
/// thinking-capability overlays the response published.
///
/// This is the same parse the probe performs, exposed so a caller can
/// retain a published contract without a network round trip. The overlay
/// is **absent** — and the returned aggregate empty — when the response
/// carries no contract.
pub fn parse_compatible_models_response_with_overlays(
    body: &[u8],
    options: &EggpoolProbeOptions,
) -> Result<(Vec<EggpoolModelSummary>, EggpoolCapabilityOverlay), EggpoolProbeError> {
    let summary = parse_summary(body, options)?;
    Ok((summary.models, summary.capability_overlay))
}

/// An Eggfetch-backed Eggpool probe.  The API key is private and has no public
/// accessor, preventing accidental inclusion in redacted result objects.
pub struct EggpoolProbe {
    base_url: String,
    api_key: EggpoolApiKey,
    options: EggpoolProbeOptions,
    client: eggfetch_core::Client,
}

impl fmt::Debug for EggpoolProbe {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("EggpoolProbe")
            .field("base_url", &self.base_url)
            .field("api_key", &self.api_key)
            .field("options", &self.options)
            .finish_non_exhaustive()
    }
}

impl EggpoolProbe {
    pub fn new(
        base_url: impl AsRef<str>,
        api_key: impl Into<EggpoolApiKey>,
        options: EggpoolProbeOptions,
    ) -> Result<Self, EggpoolProbeError> {
        validate_options(&options)?;
        let base_url = normalize_eggpool_base_url(base_url.as_ref())?;
        let api_key = api_key.into();
        if api_key.0.chars().any(char::is_control) {
            return Err(EggpoolProbeError::new(EggpoolProbeReasonCode::InvalidInput));
        }
        let client = eggfetch_core::Client::builder()
            .timeout(
                eggfetch_core::Timeout::builder()
                    .connect(options.connect_timeout)
                    .total(options.request_timeout)
                    .build(),
            )
            .follow_redirects(false)
            .build();

        Ok(Self {
            base_url,
            api_key,
            options,
            client,
        })
    }

    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    pub async fn probe(
        &self,
        cancellation: &EggpoolCancellationToken,
    ) -> Result<EggpoolProbeSummary, EggpoolProbeError> {
        match tokio::time::timeout(self.options.overall_timeout, self.probe_inner(cancellation))
            .await
        {
            Ok(result) => result,
            Err(_) => Err(EggpoolProbeError::new(EggpoolProbeReasonCode::Timeout)),
        }
    }

    async fn probe_inner(
        &self,
        cancellation: &EggpoolCancellationToken,
    ) -> Result<EggpoolProbeSummary, EggpoolProbeError> {
        if cancellation.is_cancelled() {
            return Err(EggpoolProbeError::new(EggpoolProbeReasonCode::Cancelled));
        }

        let url = format!("{}/models", self.base_url);
        let mut request = self
            .client
            .get(&url)
            .map_err(|_| EggpoolProbeError::new(EggpoolProbeReasonCode::InvalidInput))?
            .max_decoded_body_size(self.options.response_byte_limit);
        if !self.api_key.0.is_empty() {
            let value = format!("Bearer {}", self.api_key.0);
            request = request
                .header("authorization", &value)
                .header("accept", "application/json");
        }

        let mut response = select_cancel(cancellation, request.send()).await?;

        if cancellation.is_cancelled() {
            return Err(EggpoolProbeError::new(EggpoolProbeReasonCode::Cancelled));
        }

        let status = response.status();
        if status == http::StatusCode::UNAUTHORIZED
            || status == http::StatusCode::FORBIDDEN
            || status == http::StatusCode::PROXY_AUTHENTICATION_REQUIRED
        {
            return Err(EggpoolProbeError::new(EggpoolProbeReasonCode::Auth));
        }
        if status.is_redirection() {
            return Err(EggpoolProbeError::new(EggpoolProbeReasonCode::Redirect));
        }
        if !status.is_success() {
            return Err(EggpoolProbeError::new(EggpoolProbeReasonCode::Unsupported));
        }

        if response
            .content_length()
            .is_some_and(|length| length > self.options.response_byte_limit as u64)
        {
            return Err(EggpoolProbeError::new(EggpoolProbeReasonCode::Oversized));
        }

        let body = collect_body_cancellable(&mut response, cancellation).await?;
        parse_summary(&body, &self.options)
    }
}

/// Provider-neutral strict compatible `/models` probe.
///
/// Uses [`normalize_compatible_base_url`]: explicit `http(s)` endpoint,
/// preserved port/path, never injects the Eggpool preset `:11300`.
/// Transport, redirect, body/count/string bounds, JSON normalization, and
/// digest semantics are identical to [`EggpoolProbe`]; only endpoint
/// normalization differs. New generic code must use this type (or its
/// `CompatibleModelsProbe` alias), never `EggpoolProbe` directly.
pub struct CompatibleProbe {
    base_url: String,
    api_key: EggpoolApiKey,
    options: EggpoolProbeOptions,
    client: eggfetch_core::Client,
}

impl fmt::Debug for CompatibleProbe {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CompatibleProbe")
            .field("base_url", &self.base_url)
            .field("api_key", &self.api_key)
            .field("options", &self.options)
            .finish_non_exhaustive()
    }
}

impl CompatibleProbe {
    pub fn new(
        base_url: impl AsRef<str>,
        api_key: impl Into<EggpoolApiKey>,
        options: EggpoolProbeOptions,
    ) -> Result<Self, EggpoolProbeError> {
        validate_options(&options)?;
        let base_url = normalize_compatible_base_url(base_url.as_ref())?;
        let api_key = api_key.into();
        if api_key.0.chars().any(char::is_control) {
            return Err(EggpoolProbeError::new(EggpoolProbeReasonCode::InvalidInput));
        }
        let client = eggfetch_core::Client::builder()
            .timeout(
                eggfetch_core::Timeout::builder()
                    .connect(options.connect_timeout)
                    .total(options.request_timeout)
                    .build(),
            )
            .follow_redirects(false)
            .build();

        Ok(Self {
            base_url,
            api_key,
            options,
            client,
        })
    }

    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    pub async fn probe(
        &self,
        cancellation: &EggpoolCancellationToken,
    ) -> Result<EggpoolProbeSummary, EggpoolProbeError> {
        match tokio::time::timeout(self.options.overall_timeout, self.probe_inner(cancellation))
            .await
        {
            Ok(result) => result,
            Err(_) => Err(EggpoolProbeError::new(EggpoolProbeReasonCode::Timeout)),
        }
    }

    async fn probe_inner(
        &self,
        cancellation: &EggpoolCancellationToken,
    ) -> Result<EggpoolProbeSummary, EggpoolProbeError> {
        if cancellation.is_cancelled() {
            return Err(EggpoolProbeError::new(EggpoolProbeReasonCode::Cancelled));
        }

        let url = format!("{}/models", self.base_url);
        let mut request = self
            .client
            .get(&url)
            .map_err(|_| EggpoolProbeError::new(EggpoolProbeReasonCode::InvalidInput))?
            .max_decoded_body_size(self.options.response_byte_limit);
        if !self.api_key.0.is_empty() {
            let value = format!("Bearer {}", self.api_key.0);
            request = request
                .header("authorization", &value)
                .header("accept", "application/json");
        }

        let mut response = select_cancel(cancellation, request.send()).await?;

        if cancellation.is_cancelled() {
            return Err(EggpoolProbeError::new(EggpoolProbeReasonCode::Cancelled));
        }

        let status = response.status();
        if status == http::StatusCode::UNAUTHORIZED
            || status == http::StatusCode::FORBIDDEN
            || status == http::StatusCode::PROXY_AUTHENTICATION_REQUIRED
        {
            return Err(EggpoolProbeError::new(EggpoolProbeReasonCode::Auth));
        }
        if status.is_redirection() {
            return Err(EggpoolProbeError::new(EggpoolProbeReasonCode::Redirect));
        }
        if !status.is_success() {
            return Err(EggpoolProbeError::new(EggpoolProbeReasonCode::Unsupported));
        }

        if response
            .content_length()
            .is_some_and(|length| length > self.options.response_byte_limit as u64)
        {
            return Err(EggpoolProbeError::new(EggpoolProbeReasonCode::Oversized));
        }

        let body = collect_body_cancellable(&mut response, cancellation).await?;
        parse_summary(&body, &self.options)
    }
}

// ── Credential verification (bounded real completion) ──────────────
//
// `CompatibleProbe` above discovers a catalog. It does **not** verify a
// credential, and this half of the module exists because that distinction is
// not cosmetic: several real `/models` endpoints answer `200` to an
// unauthenticated reader, so a catalog 2xx says the *catalog* is usable and
// nothing at all about the key. Only a real completion can reject a credential
// that a rotation just installed, so promotion to `verified` requires one.

/// Smallest completion budget a credential-verification request may ask for.
///
/// One token is enough to prove the credential can *start* a real completion —
/// exactly the fact a catalog probe cannot establish — and keeps the check free.
pub const CREDENTIAL_PROBE_MAX_TOKENS: usize = 1;

/// Prompt used by the verification completion.
///
/// Fixed and meaningless on purpose: the verdict is carried entirely by the
/// status line, so the text must never be able to influence one.
const CREDENTIAL_PROBE_PROMPT: &str = "ping";

/// Response-body bound for the verification request.
///
/// The status line is the whole verdict, so nothing is retained and no model
/// output can leak into a log line or a durable row.
const CREDENTIAL_PROBE_BYTE_LIMIT: usize = 8 * 1024;

const DEFAULT_CREDENTIAL_REQUEST_TIMEOUT: Duration = Duration::from_secs(10);
const DEFAULT_CREDENTIAL_OVERALL_TIMEOUT: Duration = Duration::from_secs(12);

/// Verdict of one bounded credential-verification completion.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CredentialVerificationOutcome {
    /// A real completion accepted the credential. This is the **only** positive
    /// evidence that may promote a connection to `verified`.
    Accepted,
    /// A real completion rejected the credential with a typed 401/403/407.
    Rejected,
    /// The completion could not be judged: transport failure, timeout, a
    /// non-authentication status, or a surface this probe cannot build.
    ///
    /// Says nothing whatsoever about the credential. It must never be read as
    /// either acceptance or rejection — a network blip is not a bad key.
    Inconclusive,
}

/// Bounded timeouts for the credential-verification completion.
///
/// Deliberately tighter than the catalog probe's budget: the catalog request is
/// a cheap metadata read, while this one spends a real (if minimal)
/// completion against the user's account.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CredentialProbeOptions {
    pub connect_timeout: Duration,
    pub request_timeout: Duration,
    pub overall_timeout: Duration,
}

impl Default for CredentialProbeOptions {
    fn default() -> Self {
        Self {
            connect_timeout: DEFAULT_CONNECT_TIMEOUT,
            request_timeout: DEFAULT_CREDENTIAL_REQUEST_TIMEOUT,
            overall_timeout: DEFAULT_CREDENTIAL_OVERALL_TIMEOUT,
        }
    }
}

/// Where one verification request is actually sent.
///
/// Resolved before any socket is opened. For a provider the shared profile
/// adapts, this is the profile's own model → surface → URL/auth resolution; for
/// a plain OpenAI-compatible endpoint it is `{base}/chat/completions` with a
/// bearer credential, matching the catalog probe on the same connection.
#[derive(Debug, Clone)]
struct VerificationRoute {
    url: String,
    surface: eggpool_wire::profile::WireSurface,
    auth: crate::provider_profile::RouteAuth,
    static_headers: Vec<(String, String)>,
}

/// Bounded, credential-verifying completion probe for a compatible endpoint.
///
/// Sibling of [`CompatibleProbe`] rather than a mode of it: catalog discovery
/// and credential verification are different questions with different evidence
/// and different failure handling, and collapsing them is what let a dead key
/// report itself as verified.
pub struct CompatibleCredentialProbe {
    base_url: String,
    api_key: EggpoolApiKey,
    /// CodeGG provider id, when the connection is bound to a known provider.
    ///
    /// `None` (or an id the shared profile does not adapt) selects the generic
    /// OpenAI-compatible route. An adapted provider whose model has no reviewed
    /// wire mapping yields [`CredentialVerificationOutcome::Inconclusive`] —
    /// never a silent default to Chat.
    provider_id: Option<String>,
    options: CredentialProbeOptions,
    client: eggfetch_core::Client,
}

impl fmt::Debug for CompatibleCredentialProbe {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CompatibleCredentialProbe")
            .field("base_url", &self.base_url)
            .field("api_key", &self.api_key)
            .field("provider_id", &self.provider_id)
            .field("options", &self.options)
            .finish_non_exhaustive()
    }
}

impl CompatibleCredentialProbe {
    pub fn new(
        base_url: impl AsRef<str>,
        api_key: impl Into<EggpoolApiKey>,
        provider_id: Option<&str>,
        options: CredentialProbeOptions,
    ) -> Result<Self, EggpoolProbeError> {
        if options.connect_timeout.is_zero()
            || options.request_timeout.is_zero()
            || options.overall_timeout.is_zero()
        {
            return Err(EggpoolProbeError::new(EggpoolProbeReasonCode::InvalidInput));
        }
        let base_url = normalize_compatible_base_url(base_url.as_ref())?;
        let api_key = api_key.into();
        if api_key.0.chars().any(char::is_control) {
            return Err(EggpoolProbeError::new(EggpoolProbeReasonCode::InvalidInput));
        }
        let client = eggfetch_core::Client::builder()
            .timeout(
                eggfetch_core::Timeout::builder()
                    .connect(options.connect_timeout)
                    .total(options.request_timeout)
                    .build(),
            )
            .follow_redirects(false)
            .build();
        Ok(Self {
            base_url,
            api_key,
            provider_id: provider_id.map(ToOwned::to_owned),
            options,
            client,
        })
    }

    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    /// Run one minimal completion and report what it proved about the key.
    ///
    /// `model` must come from a catalog this connection already discovered. The
    /// only hard error is cancellation; every other failure mode is reported as
    /// [`CredentialVerificationOutcome::Inconclusive`] so an unreachable or
    /// slow provider can never be mistaken for an invalid credential.
    pub async fn verify(
        &self,
        model: &str,
        cancellation: &EggpoolCancellationToken,
    ) -> Result<CredentialVerificationOutcome, EggpoolProbeError> {
        if cancellation.is_cancelled() {
            return Err(EggpoolProbeError::new(EggpoolProbeReasonCode::Cancelled));
        }
        // A connection with no credential has nothing for this probe to prove;
        // sending an empty bearer would report an anonymous connection as
        // credential-failed, which is a different fact entirely.
        if self.api_key.0.is_empty() {
            return Ok(CredentialVerificationOutcome::Inconclusive);
        }
        let Some(route) = self.route(model) else {
            return Ok(CredentialVerificationOutcome::Inconclusive);
        };
        let body = match encode_verification_body(model, route.surface) {
            Ok(body) => body,
            Err(_) => return Ok(CredentialVerificationOutcome::Inconclusive),
        };
        match tokio::time::timeout(
            self.options.overall_timeout,
            self.verify_inner(&route, body, cancellation),
        )
        .await
        {
            Ok(result) => result,
            Err(_) => Ok(CredentialVerificationOutcome::Inconclusive),
        }
    }

    async fn verify_inner(
        &self,
        route: &VerificationRoute,
        body: Value,
        cancellation: &EggpoolCancellationToken,
    ) -> Result<CredentialVerificationOutcome, EggpoolProbeError> {
        let mut request = self
            .client
            .post(&route.url)
            .map_err(|_| EggpoolProbeError::new(EggpoolProbeReasonCode::InvalidInput))?
            .max_decoded_body_size(CREDENTIAL_PROBE_BYTE_LIMIT)
            .header("content-type", "application/json");
        request = match credential_header(&route.auth, &self.api_key.0) {
            Ok((name, value)) => request.header(&name, &value),
            Err(reason) => return Err(EggpoolProbeError::new(reason)),
        };
        for (name, value) in &route.static_headers {
            request = request.header(name.as_str(), value.as_str());
        }
        request = request
            .json(&body)
            .map_err(|_| EggpoolProbeError::new(EggpoolProbeReasonCode::InvalidInput))?;

        let response = select_cancel(cancellation, request.send()).await?;
        if cancellation.is_cancelled() {
            return Err(EggpoolProbeError::new(EggpoolProbeReasonCode::Cancelled));
        }
        let status = response.status();
        if status == http::StatusCode::UNAUTHORIZED
            || status == http::StatusCode::FORBIDDEN
            || status == http::StatusCode::PROXY_AUTHENTICATION_REQUIRED
        {
            return Ok(CredentialVerificationOutcome::Rejected);
        }
        if status.is_success() {
            return Ok(CredentialVerificationOutcome::Accepted);
        }
        // Everything else — 404 because the model is not on this surface, 429,
        // 5xx — is a statement about the request, not the credential.
        Ok(CredentialVerificationOutcome::Inconclusive)
    }

    /// Resolve the single surface this verification request may target.
    ///
    /// `None` means "no reviewed route exists", which is reported as
    /// inconclusive rather than guessed around.
    fn route(&self, model: &str) -> Option<VerificationRoute> {
        use eggpool_wire::profile::WireSurface;

        // An unknown provider identity is not an unresolved route — it is a
        // plain OpenAI-compatible endpoint, which is exactly what the catalog
        // probe on the same connection already speaks.
        let Some(provider_id) = self.provider_id.as_deref() else {
            return Some(self.compatible_route());
        };
        // Only a provider the shared profile actually adapts may resolve a
        // route; everything else uses the generic compatible surface below.
        if crate::provider_profile::shared_provider_id(provider_id).is_none() {
            return Some(self.compatible_route());
        }
        let route = crate::provider_profile::resolve_route(provider_id, model).ok()?;
        // Only the finite request surfaces can carry a minimal completion.
        match route.surface {
            WireSurface::OpenaiChatCompletions
            | WireSurface::OpenaiResponses
            | WireSurface::AnthropicMessages => {}
            _ => return None,
        }
        Some(VerificationRoute {
            url: route.url,
            surface: route.surface,
            auth: route.auth,
            static_headers: route.static_headers,
        })
    }

    /// Generic OpenAI-compatible route for an endpoint with no shared profile.
    fn compatible_route(&self) -> VerificationRoute {
        use eggpool_wire::profile::WireSurface;
        VerificationRoute {
            url: format!("{}/chat/completions", self.base_url),
            surface: WireSurface::OpenaiChatCompletions,
            // Same shape the catalog probe presents on this connection, so the
            // two halves of one probe never disagree about the key.
            auth: crate::provider_profile::RouteAuth::Bearer {
                header: "authorization".to_owned(),
                scheme: "Bearer".to_owned(),
            },
            static_headers: Vec::new(),
        }
    }
}

/// Render the resolved surface's credential shape into a header pair.
///
/// Rejects a surface that declares no credential rather than silently sending
/// an unauthenticated request that would read as a rejection.
fn credential_header(
    auth: &crate::provider_profile::RouteAuth,
    secret: &str,
) -> Result<(String, String), EggpoolProbeReasonCode> {
    use crate::provider_profile::RouteAuth;
    let (name, value) = match auth {
        RouteAuth::Bearer { header, scheme } => (header.clone(), format!("{scheme} {secret}")),
        RouteAuth::ApiKeyHeader { header } | RouteAuth::RawAuthorization { header } => {
            (header.clone(), secret.to_owned())
        }
        RouteAuth::None => return Err(EggpoolProbeReasonCode::InvalidInput),
    };
    if value.chars().any(|character| character.is_control()) {
        return Err(EggpoolProbeReasonCode::InvalidInput);
    }
    Ok((name, value))
}

/// Build the smallest request the resolved surface accepts.
///
/// Mirrors the surface dispatch every streaming provider uses, so a model on
/// Anthropic Messages or OpenAI Responses is verified through the grammar it
/// actually runs on rather than a hardcoded Chat body that the surface would
/// reject before it ever evaluated the credential.
fn encode_verification_body(
    model: &str,
    surface: eggpool_wire::profile::WireSurface,
) -> Result<Value, crate::ProviderError> {
    use crate::provider_core::{ChatRequest, ContentPart, Message};
    use eggpool_wire::profile::WireSurface;

    let request = ChatRequest {
        messages: vec![Message::User {
            content: vec![ContentPart::Text {
                text: Arc::new(CREDENTIAL_PROBE_PROMPT.to_owned()),
            }],
        }],
        model: model.to_owned(),
        tools: None,
        system: None,
        temperature: None,
        top_p: None,
        max_tokens: Some(CREDENTIAL_PROBE_MAX_TOKENS),
        response_format: None,
        thinking_budget: None,
        reasoning_effort: None,
        context: Default::default(),
    };
    match surface {
        WireSurface::OpenaiChatCompletions => {
            crate::wire::encode_openai_chat(&request, None, false)
        }
        WireSurface::OpenaiResponses => crate::wire::encode_openai_responses(&request, false),
        WireSurface::AnthropicMessages => crate::wire::encode_anthropic_messages(&request),
        other => Err(crate::ProviderError::api(
            "wire_surface_unsupported",
            format!("wire surface {other:?} cannot carry a verification completion"),
        )),
    }
}

fn validate_options(options: &EggpoolProbeOptions) -> Result<(), EggpoolProbeError> {
    if options.connect_timeout.is_zero()
        || options.request_timeout.is_zero()
        || options.overall_timeout.is_zero()
        || options.response_byte_limit == 0
        || options.model_count_limit == 0
        || options.model_string_limit == 0
    {
        return Err(EggpoolProbeError::new(EggpoolProbeReasonCode::InvalidInput));
    }
    Ok(())
}

fn has_path_traversal(candidate: &str) -> bool {
    let Some((_, authority_and_path)) = candidate.split_once("://") else {
        return false;
    };
    let Some(path_start) = authority_and_path.find('/') else {
        return false;
    };
    let path = authority_and_path[path_start..]
        .split(['?', '#'])
        .next()
        .unwrap_or_default();
    path.split('/').any(|segment| {
        segment == ".."
            || segment.eq_ignore_ascii_case("%2e%2e")
            || segment.eq_ignore_ascii_case("%2e.%2e")
            || segment.eq_ignore_ascii_case(".%2e")
    })
}

async fn select_cancel<F, T>(
    cancellation: &EggpoolCancellationToken,
    operation: F,
) -> Result<T, EggpoolProbeError>
where
    F: std::future::Future<Output = Result<T, eggfetch_core::Error>>,
{
    tokio::select! {
        _ = cancellation.cancelled() => Err(EggpoolProbeError::new(EggpoolProbeReasonCode::Cancelled)),
        result = operation => result.map_err(classify_request_error),
    }
}

async fn collect_body_cancellable(
    response: &mut eggfetch_core::Response,
    cancellation: &EggpoolCancellationToken,
) -> Result<Vec<u8>, EggpoolProbeError> {
    tokio::select! {
        _ = cancellation.cancelled() => Err(EggpoolProbeError::new(EggpoolProbeReasonCode::Cancelled)),
        result = response.bytes() => result
            .map(|bytes| bytes.to_vec())
            .map_err(classify_body_error),
    }
}

fn classify_request_error(error: eggfetch_core::Error) -> EggpoolProbeError {
    if matches!(error, eggfetch_core::Error::Timeout { .. }) {
        return EggpoolProbeError::new(EggpoolProbeReasonCode::Timeout);
    }
    if looks_like_tls_error(&error) {
        return EggpoolProbeError::new(EggpoolProbeReasonCode::Tls);
    }
    EggpoolProbeError::new(EggpoolProbeReasonCode::Unreachable)
}

fn classify_body_error(error: eggfetch_core::Error) -> EggpoolProbeError {
    if matches!(error, eggfetch_core::Error::DecodedBodyTooLarge) {
        return EggpoolProbeError::new(EggpoolProbeReasonCode::Oversized);
    }
    classify_request_error(error)
}

fn looks_like_tls_error(error: &eggfetch_core::Error) -> bool {
    matches!(
        error.kind(),
        "tls"
            | "tls_config"
            | "certificate_verification"
            | "hostname_verification"
            | "ca_bundle"
            | "client_cert"
            | "private_key"
    )
}

fn parse_summary(
    body: &[u8],
    options: &EggpoolProbeOptions,
) -> Result<EggpoolProbeSummary, EggpoolProbeError> {
    let value: Value = serde_json::from_slice(body)
        .map_err(|_| EggpoolProbeError::new(EggpoolProbeReasonCode::InvalidJson))?;
    let data = value
        .get("data")
        .and_then(Value::as_array)
        .ok_or_else(|| EggpoolProbeError::new(EggpoolProbeReasonCode::Unsupported))?;

    if data.is_empty() {
        return Err(EggpoolProbeError::new(EggpoolProbeReasonCode::Empty));
    }
    if data.len() > options.model_count_limit {
        return Err(EggpoolProbeError::new(EggpoolProbeReasonCode::Oversized));
    }

    let mut models = BTreeMap::new();
    let mut overlays: BTreeMap<String, OverlayCandidate> = BTreeMap::new();
    for item in data {
        let id = item
            .get("id")
            .and_then(Value::as_str)
            .filter(|id| !id.is_empty())
            .ok_or_else(|| EggpoolProbeError::new(EggpoolProbeReasonCode::Unsupported))?;
        let name = item.get("name").and_then(Value::as_str).unwrap_or(id);
        if id.chars().count() > options.model_string_limit
            || name.chars().count() > options.model_string_limit
        {
            return Err(EggpoolProbeError::new(EggpoolProbeReasonCode::Oversized));
        }

        // Duplicate IDs are one catalog entry.  BTreeMap ordering gives a
        // stable winner when a server returns duplicate IDs with names.
        models
            .entry(id.to_owned())
            .and_modify(|existing: &mut String| {
                if name < existing.as_str() {
                    *existing = name.to_owned();
                }
            })
            .or_insert_with(|| name.to_owned());

        // The published thinking-capability contract is **retained**, not
        // discarded. It is held apart from the model summary so the
        // catalog digest keeps hashing `id` and `name` only.
        let overlay = parse_thinking_overlay(item);
        match overlays.entry(id.to_owned()) {
            Entry::Vacant(slot) => {
                slot.insert(match overlay {
                    Some(overlay) => OverlayCandidate::One(overlay),
                    None => OverlayCandidate::Absent,
                });
            }
            Entry::Occupied(mut slot) => {
                let existing = slot.insert(OverlayCandidate::Absent);
                slot.insert(merge_overlay_candidate(existing, overlay));
            }
        }
    }

    if models.is_empty() {
        return Err(EggpoolProbeError::new(EggpoolProbeReasonCode::Empty));
    }

    let models = models
        .into_iter()
        .map(|(id, name)| EggpoolModelSummary { id, name })
        .collect::<Vec<_>>();
    let digest = digest_models(&models);
    let capability_overlay = EggpoolCapabilityOverlay::new(
        overlays
            .into_iter()
            .filter_map(|(model_id, candidate)| match candidate {
                OverlayCandidate::One(overlay) => Some((model_id, overlay)),
                OverlayCandidate::Absent | OverlayCandidate::Conflicting => None,
            })
            .collect(),
    );
    Ok(EggpoolProbeSummary {
        models,
        digest,
        capability_overlay,
    })
}

/// The overlay candidates seen for one model id.
///
/// A duplicate id is one catalog entry, so two different contracts for the
/// same id are ambiguous and neither is retained.
enum OverlayCandidate {
    /// No contract was published for this id.
    Absent,
    /// Exactly one contract was published for this id.
    One(EggpoolThinkingOverlay),
    /// Two different contracts were published for this id.
    Conflicting,
}

/// Merge a newly seen contract into the candidates for one model id.
///
/// The merge is order-independent: one contract wins whether it was seen
/// first or last, and two different contracts leave the id without an
/// overlay.
fn merge_overlay_candidate(
    existing: OverlayCandidate,
    incoming: Option<EggpoolThinkingOverlay>,
) -> OverlayCandidate {
    match (existing, incoming) {
        (OverlayCandidate::Conflicting, _) => OverlayCandidate::Conflicting,
        (OverlayCandidate::Absent, None) => OverlayCandidate::Absent,
        (OverlayCandidate::Absent, Some(overlay)) => OverlayCandidate::One(overlay),
        (OverlayCandidate::One(current), None) => OverlayCandidate::One(current),
        (OverlayCandidate::One(current), Some(incoming)) => {
            if current == incoming {
                OverlayCandidate::One(current)
            } else {
                OverlayCandidate::Conflicting
            }
        }
    }
}

/// Compute the deterministic catalog revision for a bounded normalized model
/// list. Callers may reuse this helper during refresh without changing the
/// catalog revision when the provider returns the same content in another
/// order.
pub fn digest_models(models: &[EggpoolModelSummary]) -> String {
    let mut hasher = Sha256::new();
    for model in models {
        hash_string(&mut hasher, &model.id);
        hash_string(&mut hasher, &model.name);
    }
    hex::encode(hasher.finalize())
}

fn hash_string(hasher: &mut Sha256, value: &str) {
    hasher.update((value.len() as u64).to_be_bytes());
    hasher.update(value.as_bytes());
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::net::{TcpListener, TcpStream};
    use std::sync::OnceLock;
    use std::thread;

    /// An unknown provider identity is a plain OpenAI-compatible endpoint, so
    /// verification must fall back to the generic Chat route rather than
    /// declaring itself unable to verify anything.
    #[test]
    fn verification_route_falls_back_to_chat_for_an_unknown_provider() {
        for provider_id in [None, Some("custom"), Some("eggpool")] {
            let probe = CompatibleCredentialProbe::new(
                "http://127.0.0.1:9/v1",
                EggpoolApiKey::new("k"),
                provider_id,
                CredentialProbeOptions::default(),
            )
            .expect("probe builds");
            let route = probe
                .route("some-model")
                .expect("a compatible route exists");
            assert_eq!(route.url, "http://127.0.0.1:9/v1/chat/completions");
            assert_eq!(
                route.surface,
                eggpool_wire::profile::WireSurface::OpenaiChatCompletions
            );
        }
    }

    /// A provider the shared profile *does* adapt must resolve through the
    /// profile — and a model with no reviewed wire mapping must fail closed
    /// rather than silently defaulting to Chat.
    #[test]
    fn verification_route_fails_closed_for_an_unresolved_adapted_model() {
        let probe = CompatibleCredentialProbe::new(
            "http://127.0.0.1:9/v1",
            EggpoolApiKey::new("k"),
            Some(crate::opencode_go::PROVIDER_ID),
            CredentialProbeOptions::default(),
        )
        .expect("probe builds");
        assert!(
            probe.route("no-such-model-in-the-profile").is_none(),
            "an unreviewed model must not be guessed onto Chat"
        );
    }

    struct FakeServer {
        url: String,
        join: thread::JoinHandle<(String, String)>,
    }

    static FAKE_SERVER_TEST_LOCK: OnceLock<tokio::sync::Mutex<()>> = OnceLock::new();

    async fn fake_server_test_lock() -> tokio::sync::MutexGuard<'static, ()> {
        FAKE_SERVER_TEST_LOCK
            .get_or_init(|| tokio::sync::Mutex::new(()))
            .lock()
            .await
    }

    fn fake_server(status: u16, body: &'static str, delay: Duration) -> Option<FakeServer> {
        let listener = match TcpListener::bind(("127.0.0.1", 0)) {
            Ok(listener) => listener,
            // Some restricted test sandboxes disallow local listeners.  The
            // deterministic integration coverage remains active where local
            // sockets are available.
            Err(_) => return None,
        };
        let address = listener.local_addr().expect("fake server address");
        let join = thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("accept fake request");
            let request = read_request(&mut stream);
            if !delay.is_zero() {
                thread::sleep(delay);
            }
            let reason = if status == 200 { "OK" } else { "ERROR" };
            let response = format!(
                "HTTP/1.1 {status} {reason}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            let _ = stream.write_all(response.as_bytes());
            let auth = request
                .lines()
                .find_map(|line| {
                    let (name, value) = line.split_once(':')?;
                    name.eq_ignore_ascii_case("authorization").then_some(value)
                })
                .unwrap_or_default()
                .trim()
                .to_owned();
            let path = request
                .lines()
                .next()
                .and_then(|line| line.split_whitespace().nth(1))
                .unwrap_or_default()
                .to_owned();
            (auth, path)
        });
        Some(FakeServer {
            url: format!("http://{address}"),
            join,
        })
    }

    fn read_request(stream: &mut TcpStream) -> String {
        let mut bytes = Vec::new();
        let mut chunk = [0_u8; 1024];
        loop {
            let count = stream.read(&mut chunk).expect("read fake request");
            if count == 0 {
                break;
            }
            bytes.extend_from_slice(&chunk[..count]);
            if bytes.windows(4).any(|window| window == b"\r\n\r\n") {
                break;
            }
        }
        String::from_utf8(bytes).expect("fake request is HTTP text")
    }

    fn probe(server: &FakeServer, key: &str, options: EggpoolProbeOptions) -> EggpoolProbe {
        EggpoolProbe::new(&server.url, key, options).expect("valid probe")
    }

    #[test]
    fn normalizes_host_port_and_rejects_secret_bearing_urls() {
        assert_eq!(
            normalize_eggpool_base_url("127.0.0.1"),
            Ok("http://127.0.0.1:11300".to_string())
        );
        assert_eq!(
            normalize_eggpool_base_url("https://[::1]:1234/v1/"),
            Ok("https://[::1]:1234/v1".to_string())
        );
        for value in [
            "https://user:pass@example.test",
            "https://example.test?key=secret",
            "https://example.test/../private",
            "http://example.test:0",
            "ftp://example.test",
        ] {
            assert_eq!(
                normalize_eggpool_base_url(value)
                    .expect_err("input should be rejected")
                    .reason_code(),
                EggpoolProbeReasonCode::InvalidInput
            );
        }
    }

    #[test]
    fn generic_compatible_preserves_port_path_and_never_injects_default_port() {
        // Explicit ports and paths are preserved exactly (trailing slash trimmed).
        assert_eq!(
            normalize_compatible_base_url("https://api.example.test/v1"),
            Ok("https://api.example.test/v1".to_string())
        );
        assert_eq!(
            normalize_compatible_base_url("https://api.example.test/v1/"),
            Ok("https://api.example.test/v1".to_string())
        );
        assert_eq!(
            normalize_compatible_base_url("https://[::1]:1234/v1/"),
            Ok("https://[::1]:1234/v1".to_string())
        );
        assert_eq!(
            normalize_compatible_base_url("http://127.0.0.1:8080/custom/prefix/"),
            Ok("http://127.0.0.1:8080/custom/prefix".to_string())
        );
        // No port stays no port: generic validation never injects :11300.
        assert_eq!(
            normalize_compatible_base_url("https://api.example.test/v1"),
            Ok("https://api.example.test/v1".to_string())
        );
        assert!(
            !normalize_compatible_base_url("https://api.example.test/v1")
                .expect("valid generic endpoint")
                .contains(":11300")
        );
        // Host-only shorthand and non-HTTP(S) inputs are invalid for generic
        // endpoints; they belong to the Eggpool preset only.
        for value in [
            "127.0.0.1",
            "example.test",
            "api.example.test/v1",
            "ftp://example.test/v1",
            "https://user:pass@example.test",
            "https://example.test?key=secret",
            "https://example.test#fragment",
            "https://example.test/../private",
            "http://example.test:0",
            "",
            "https://example.test/v1\u{7}",
        ] {
            assert_eq!(
                normalize_compatible_base_url(value)
                    .expect_err("generic input should be rejected")
                    .reason_code(),
                EggpoolProbeReasonCode::InvalidInput,
                "unexpected acceptance for {value:?}"
            );
        }
    }

    #[test]
    fn compatible_probe_uses_generic_semantics_while_eggpool_keeps_preset() {
        // Same origin without a port: generic preserves, preset injects.
        let generic =
            normalize_compatible_base_url("https://api.example.test/v1").expect("generic valid");
        let preset =
            normalize_eggpool_base_url("https://api.example.test/v1").expect("preset valid");
        assert_eq!(generic, "https://api.example.test/v1");
        assert_eq!(preset, "https://api.example.test:11300/v1");

        // CompatibleProbe accepts explicit endpoints and preserves them.
        let compatible = CompatibleProbe::new(
            "https://api.example.test/v1",
            "key",
            EggpoolProbeOptions::default(),
        )
        .expect("compatible probe builds");
        assert_eq!(compatible.base_url(), "https://api.example.test/v1");

        // CompatibleProbe rejects host-only shorthand.
        assert_eq!(
            CompatibleProbe::new("127.0.0.1", "key", EggpoolProbeOptions::default())
                .expect_err("host-only must fail for generic")
                .reason_code(),
            EggpoolProbeReasonCode::InvalidInput
        );

        // EggpoolProbe still accepts host shorthand with default port.
        let preset_probe = EggpoolProbe::new("127.0.0.1", "key", EggpoolProbeOptions::default())
            .expect("preset accepts host shorthand");
        assert_eq!(preset_probe.base_url(), "http://127.0.0.1:11300");
    }

    #[test]
    fn shared_parser_enforces_bounds_with_deterministic_dedup() {
        let options = EggpoolProbeOptions::default();
        // Duplicate IDs collapse to one entry with smallest name winning.
        let body = br#"{"data":[{"id":"zeta"},{"id":"alpha","name":"Alpha"},{"id":"zeta","name":"Zeta"}]}"#;
        let models = parse_compatible_models_response(body, &options).expect("valid bounded body");
        assert_eq!(
            models,
            vec![
                EggpoolModelSummary {
                    id: "alpha".to_string(),
                    name: "Alpha".to_string(),
                },
                EggpoolModelSummary {
                    id: "zeta".to_string(),
                    name: "Zeta".to_string(),
                },
            ]
        );
        // Oversized count fails closed.
        let many: Vec<String> = (0..300).map(|i| format!(r#"{{"id":"m{i}"}}"#)).collect();
        let oversized = format!(r#"{{"data":[{}]}}"#, many.join(","));
        assert_eq!(
            parse_compatible_models_response(oversized.as_bytes(), &options)
                .expect_err("count must be bounded")
                .reason_code(),
            EggpoolProbeReasonCode::Oversized
        );
        // Oversized strings fail closed.
        let long_id = "x".repeat(300);
        let long_body = format!(r#"{{"data":[{{"id":"{long_id}"}}]}}"#);
        assert_eq!(
            parse_compatible_models_response(long_body.as_bytes(), &options)
                .expect_err("string must be bounded")
                .reason_code(),
            EggpoolProbeReasonCode::Oversized
        );
        // Invalid JSON and empty fail closed.
        for (body, expected) in [
            ("not-json".as_bytes(), EggpoolProbeReasonCode::InvalidJson),
            (br#"{"data":[]}"#.as_slice(), EggpoolProbeReasonCode::Empty),
            (
                br#"{"not":"compatible"}"#.as_slice(),
                EggpoolProbeReasonCode::Unsupported,
            ),
        ] {
            assert_eq!(
                parse_compatible_models_response(body, &options)
                    .expect_err("shape must fail")
                    .reason_code(),
                expected
            );
        }
    }

    #[tokio::test]
    async fn probes_models_with_auth_and_stable_summary() {
        let _lock = fake_server_test_lock().await;
        let Some(server) = fake_server(
            200,
            r#"{"data":[{"id":"zeta"},{"id":"alpha","name":"Alpha"},{"id":"zeta","name":"Zeta"}]}"#,
            Duration::ZERO,
        ) else {
            return;
        };
        let probe = probe(&server, "test-api-key", EggpoolProbeOptions::default());
        let cancellation = EggpoolCancellationToken::new();
        let summary = probe.probe(&cancellation).await.expect("probe succeeds");

        assert_eq!(probe.base_url(), server.url);
        assert_eq!(
            summary.models,
            vec![
                EggpoolModelSummary {
                    id: "alpha".to_string(),
                    name: "Alpha".to_string(),
                },
                EggpoolModelSummary {
                    id: "zeta".to_string(),
                    name: "Zeta".to_string(),
                },
            ]
        );
        assert_eq!(summary.digest, digest_models(&summary.models));
        let (auth, path) = server.join.join().expect("fake server joins");
        assert_eq!(auth, "Bearer test-api-key");
        assert_eq!(path, "/models");
    }

    #[tokio::test]
    async fn returns_redacted_reason_codes_for_response_shapes_and_limits() {
        let _lock = fake_server_test_lock().await;
        let cases = [
            (
                401,
                r#"{"error":"the-key-must-not-leak"}"#,
                EggpoolProbeReasonCode::Auth,
            ),
            (
                404,
                r#"{"error":"not-openai-compatible"}"#,
                EggpoolProbeReasonCode::Unsupported,
            ),
            (
                302,
                r#"{"location":"https://elsewhere.invalid"}"#,
                EggpoolProbeReasonCode::Redirect,
            ),
            (200, "not-json", EggpoolProbeReasonCode::InvalidJson),
            (200, r#"{"data":[]}"#, EggpoolProbeReasonCode::Empty),
        ];
        for (status, body, expected) in cases {
            let Some(server) = fake_server(status, body, Duration::ZERO) else {
                return;
            };
            let probe = probe(
                &server,
                "the-key-must-not-leak",
                EggpoolProbeOptions::default(),
            );
            let error = probe
                .probe(&EggpoolCancellationToken::new())
                .await
                .expect_err("probe should fail");
            assert_eq!(error.reason_code(), expected);
            assert!(!error.to_string().contains("the-key-must-not-leak"));
            server.join.join().expect("fake server joins");
        }

        let Some(server) = fake_server(200, r#"{"data":[{"id":"long"}]}"#, Duration::ZERO) else {
            return;
        };
        let options = EggpoolProbeOptions {
            response_byte_limit: 8,
            ..EggpoolProbeOptions::default()
        };
        let error = probe(&server, "key", options)
            .probe(&EggpoolCancellationToken::new())
            .await
            .expect_err("response should be bounded");
        assert_eq!(error.reason_code(), EggpoolProbeReasonCode::Oversized);
        server.join.join().expect("fake server joins");
    }

    #[tokio::test]
    async fn cancellation_and_overall_timeout_are_bounded() {
        let _lock = fake_server_test_lock().await;
        let Some(server) = fake_server(
            200,
            r#"{"data":[{"id":"slow"}]}"#,
            Duration::from_millis(250),
        ) else {
            return;
        };
        let options = EggpoolProbeOptions {
            overall_timeout: Duration::from_millis(25),
            request_timeout: Duration::from_secs(1),
            ..EggpoolProbeOptions::default()
        };
        let error = probe(&server, "key", options)
            .probe(&EggpoolCancellationToken::new())
            .await
            .expect_err("overall timeout should win");
        assert_eq!(error.reason_code(), EggpoolProbeReasonCode::Timeout);
        server.join.join().expect("fake server joins");

        let token = EggpoolCancellationToken::new();
        token.cancel();
        let error = EggpoolProbe::new("127.0.0.1", "key", EggpoolProbeOptions::default())
            .expect("valid probe")
            .probe(&token)
            .await
            .expect_err("cancelled probe should not request");
        assert_eq!(error.reason_code(), EggpoolProbeReasonCode::Cancelled);
    }

    #[test]
    fn secret_is_redacted_from_debug_and_display() {
        let key = EggpoolApiKey::new("super-secret-api-key");
        assert!(!format!("{key:?}").contains("super-secret-api-key"));
        assert!(!key.to_string().contains("super-secret-api-key"));
    }

    #[test]
    fn redirect_reason_is_stable_and_redacted() {
        assert_eq!(
            EggpoolProbeReasonCode::Redirect.as_str(),
            "redirect_disallowed"
        );
        let error = EggpoolProbeError::new(EggpoolProbeReasonCode::Redirect);
        assert!(!error.to_string().contains("Location"));
    }

    async fn chunked_probe_server(chunks: Vec<Vec<u8>>) -> (String, tokio::task::JoinHandle<()>) {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        use tokio::net::TcpListener;

        let listener = TcpListener::bind(("127.0.0.1", 0))
            .await
            .expect("bind chunked probe server");
        let addr = listener.local_addr().expect("chunked probe address");
        let handle = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.expect("accept probe request");
            let mut request = [0_u8; 1024];
            let _ = socket.read(&mut request).await;
            socket
                .write_all(
                    b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n",
                )
                .await
                .expect("write chunked headers");
            for chunk in chunks {
                socket
                    .write_all(format!("{:x}\r\n", chunk.len()).as_bytes())
                    .await
                    .expect("write chunk size");
                socket.write_all(&chunk).await.expect("write chunk");
                socket.write_all(b"\r\n").await.expect("write chunk end");
            }
            socket
                .write_all(b"0\r\n\r\n")
                .await
                .expect("write chunk terminator");
        });
        (format!("http://{addr}"), handle)
    }

    #[tokio::test]
    async fn oversized_chunked_body_maps_to_oversized_without_content_length() {
        let _lock = fake_server_test_lock().await;
        // No Content-Length is sent; the Eggfetch request limit must still
        // fail closed and map to the stable Oversized reason.
        let body = br#"{"data":[{"id":"chunked-oversized-model"}]}"#.to_vec();
        assert!(body.len() > 16);
        let (url, server) =
            chunked_probe_server(vec![body[..16].to_vec(), body[16..].to_vec()]).await;
        let options = EggpoolProbeOptions {
            response_byte_limit: 16,
            ..EggpoolProbeOptions::default()
        };
        let error = EggpoolProbe::new(&url, "key", options)
            .expect("valid probe")
            .probe(&EggpoolCancellationToken::new())
            .await
            .expect_err("chunked over-limit must fail closed");
        assert_eq!(error.reason_code(), EggpoolProbeReasonCode::Oversized);
        server.await.expect("chunked server joins");
    }

    #[tokio::test]
    async fn cancellation_during_body_collection_wins_promptly() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        use tokio::net::TcpListener;

        let _lock = fake_server_test_lock().await;
        let listener = TcpListener::bind(("127.0.0.1", 0))
            .await
            .expect("bind stalled probe server");
        let addr = listener.local_addr().expect("stalled probe address");
        let server = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.expect("accept stalled request");
            let mut request = [0_u8; 1024];
            let _ = socket.read(&mut request).await;
            // Claim a large body but stall after a small prefix so the
            // client must remain in body collection when cancelled.
            socket
                .write_all(
                    b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 1048576\r\nConnection: close\r\n\r\n{\"data\":[",
                )
                .await
                .expect("write stalled prefix");
            tokio::time::sleep(Duration::from_secs(10)).await;
        });

        let url = format!("http://{addr}");
        let probe =
            EggpoolProbe::new(&url, "key", EggpoolProbeOptions::default()).expect("valid probe");
        let token = EggpoolCancellationToken::new();
        let canceller = token.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(100)).await;
            canceller.cancel();
        });
        let result = tokio::time::timeout(Duration::from_secs(5), probe.probe(&token)).await;
        match result {
            Ok(Err(error)) => assert_eq!(error.reason_code(), EggpoolProbeReasonCode::Cancelled),
            Ok(Ok(_)) => panic!("stalled body should not succeed"),
            Err(_) => panic!("cancellation during body collection was not prompt"),
        }
        server.abort();
    }

    // ------------------------------------------------------------------
    // The discovery overlay (ADR-0045, milestone 005).
    // ------------------------------------------------------------------

    /// A flat `thinking` capability — the shape the pinned upstream exposes
    /// at runtime, on the integrations-profile route it does not publish.
    const FLAT_THINKING_RESPONSE: &str = r#"{"data":[{"id":"m","name":"m","capabilities":{"thinking":{"status":"supported","toggle":"unsupported","effort":"supported","budget":"supported","supported_efforts":["low","medium","high"],"budget_tokens_min":1024,"budget_tokens_max":128000}}}]}"#;

    /// The documented `control_contract.accepted_efforts` shape.
    const DOCUMENTED_CONTRACT_RESPONSE: &str = r#"{"data":[{"id":"m","name":"m","eggpool":{"provider_id":"p","base_model_id":"m","capabilities":{"thinking":{"status":"supported","control_contract":{"toggle":"unsupported","effort":"supported","budget":"supported","accepted_efforts":["low","medium","high"]},"budget_tokens_min":1024,"budget_tokens_max":128000}}}}]}"#;

    /// A response with no capability block at all.
    const PLAIN_RESPONSE: &str =
        r#"{"data":[{"id":"m","name":"m","object":"model","owned_by":"eggpool"}]}"#;

    fn labels(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| (*value).to_string()).collect()
    }

    fn parse(body: &str) -> EggpoolProbeSummary {
        parse_summary(body.as_bytes(), &EggpoolProbeOptions::default())
            .expect("the response parses")
    }

    /// A response whose one model publishes the given effort ladder.
    fn response_with_ladder(model: &str, ladder: &[&str]) -> String {
        let ladder = ladder
            .iter()
            .map(|label| format!("\"{label}\""))
            .collect::<Vec<_>>()
            .join(",");
        format!(
            "{{\"data\":[{{\"id\":\"{model}\",\"name\":\"{model}\",\"thinking\":{{\"effort\":\"supported\",\"supported_efforts\":[{ladder}]}}}}]}}"
        )
    }

    #[test]
    fn the_catalog_parse_retains_a_flat_thinking_contract() {
        let summary = parse(FLAT_THINKING_RESPONSE);
        let overlay = summary
            .capability_overlay
            .for_model("m")
            .expect("the published contract is retained");
        let record = overlay.record();
        assert_eq!(record.effort_status, ReasoningCapabilityStatus::Supported);
        let expected = labels(&["high", "low", "medium"]);
        assert_eq!(
            record.accepted_effort_labels.as_deref(),
            Some(expected.as_slice())
        );
        assert_eq!(
            record.disable_status,
            ReasoningCapabilityStatus::Unsupported
        );
        assert_eq!(record.budget_status, ReasoningCapabilityStatus::Supported);
        assert_eq!(record.min_thinking_budget, Some(1024));
        assert_eq!(record.max_thinking_budget, Some(128_000));
        assert!(
            record.validate().is_empty(),
            "the retained overlay must be a well-formed record: {:?}",
            record.validate()
        );
    }

    #[test]
    fn the_catalog_parse_retains_the_documented_control_contract_shape() {
        let summary = parse(DOCUMENTED_CONTRACT_RESPONSE);
        let overlay = summary
            .capability_overlay
            .for_model("m")
            .expect("the published contract is retained");
        let record = overlay.record();
        assert_eq!(record.effort_status, ReasoningCapabilityStatus::Supported);
        let expected = labels(&["high", "low", "medium"]);
        assert_eq!(
            record.accepted_effort_labels.as_deref(),
            Some(expected.as_slice())
        );
        assert_eq!(
            record.disable_status,
            ReasoningCapabilityStatus::Unsupported
        );
        assert_eq!(record.budget_status, ReasoningCapabilityStatus::Supported);
        assert_eq!(record.min_thinking_budget, Some(1024));
        assert_eq!(record.max_thinking_budget, Some(128_000));
    }

    #[test]
    fn a_response_without_a_contract_retains_no_overlay() {
        let summary = parse(PLAIN_RESPONSE);
        assert!(
            summary.capability_overlay.is_empty(),
            "a response with no capability block carries no overlay"
        );
        assert!(summary.capability_overlay.for_model("m").is_none());
    }

    #[test]
    fn the_catalog_digest_is_unchanged_by_the_contracts_presence() {
        let plain = parse(PLAIN_RESPONSE);
        let with_flat = parse(FLAT_THINKING_RESPONSE);
        let with_documented = parse(DOCUMENTED_CONTRACT_RESPONSE);
        assert_eq!(
            plain.digest, with_flat.digest,
            "the catalog digest hashes id and name only, so a published contract must not move it"
        );
        assert_eq!(plain.digest, with_documented.digest);
        // The digest is still the digest of the models it was computed from.
        assert_eq!(plain.digest, digest_models(&plain.models));
    }

    #[test]
    fn the_capability_revision_moves_with_the_content_and_not_with_the_ordering() {
        let flat = parse(FLAT_THINKING_RESPONSE);
        let documented = parse(DOCUMENTED_CONTRACT_RESPONSE);
        // The two wire shapes express the same capability, so they share a
        // capability revision: the revision is over the capability, not
        // over the spelling the provider happened to use.
        assert_eq!(
            flat.capability_overlay.capability_revision(),
            documented.capability_overlay.capability_revision(),
        );
        assert_ne!(
            flat.capability_overlay.capability_revision(),
            parse(PLAIN_RESPONSE)
                .capability_overlay
                .capability_revision(),
            "a response with no contract must not share a revision with one that has"
        );

        let first = parse(
            r#"{"data":[{"id":"a","name":"a","thinking":{"effort":"supported","supported_efforts":["low"]}},{"id":"b","name":"b","thinking":{"effort":"supported","supported_efforts":["high"]}}]}"#,
        );
        let reordered = parse(
            r#"{"data":[{"id":"b","name":"b","thinking":{"effort":"supported","supported_efforts":["high"]}},{"id":"a","name":"a","thinking":{"effort":"supported","supported_efforts":["low"]}}]}"#,
        );
        assert_eq!(
            first.capability_overlay.capability_revision(),
            reordered.capability_overlay.capability_revision(),
            "the response's ordering must not move the capability revision"
        );
        assert_eq!(first.digest, reordered.digest);

        let changed = parse(
            r#"{"data":[{"id":"a","name":"a","thinking":{"effort":"supported","supported_efforts":["medium"]}},{"id":"b","name":"b","thinking":{"effort":"supported","supported_efforts":["high"]}}]}"#,
        );
        assert_ne!(
            first.capability_overlay.capability_revision(),
            changed.capability_overlay.capability_revision(),
            "a changed contract must move the capability revision"
        );
    }

    #[test]
    fn an_oversized_label_list_degrades_the_effort_dimension_to_unknown() {
        let oversized: Vec<String> = (0..(MAX_REASONING_CAPABILITY_LABELS + 1))
            .map(|index| format!("\"level-{index}\""))
            .collect();
        let body = format!(
            "{{\"data\":[{{\"id\":\"m\",\"name\":\"m\",\"thinking\":{{\"toggle\":\"unsupported\",\"effort\":\"supported\",\"supported_efforts\":[{}]}}}}]}}",
            oversized.join(",")
        );
        let summary = parse(&body);
        let overlay = summary
            .capability_overlay
            .for_model("m")
            .expect("the contract is retained");
        assert_eq!(
            overlay.record().effort_status,
            ReasoningCapabilityStatus::Unknown,
            "an oversized ladder must degrade the dimension, not be truncated"
        );
        assert!(overlay.record().accepted_effort_labels.is_none());
        // The dimension that was published soundly still stands.
        assert_eq!(
            overlay.record().disable_status,
            ReasoningCapabilityStatus::Unsupported
        );
    }

    #[test]
    fn an_over_long_label_degrades_the_effort_dimension_to_unknown() {
        let long = "x".repeat(MAX_REASONING_CAPABILITY_LABEL_BYTES + 1);
        let body = format!(
            "{{\"data\":[{{\"id\":\"m\",\"name\":\"m\",\"thinking\":{{\"toggle\":\"unsupported\",\"effort\":\"supported\",\"supported_efforts\":[\"{long}\"]}}}}]}}"
        );
        let summary = parse(&body);
        let overlay = summary
            .capability_overlay
            .for_model("m")
            .expect("the contract is retained");
        assert_eq!(
            overlay.record().effort_status,
            ReasoningCapabilityStatus::Unknown
        );
        assert!(overlay.record().accepted_effort_labels.is_none());
    }

    #[test]
    fn duplicate_labels_are_deduplicated() {
        let body = r#"{"data":[{"id":"m","name":"m","thinking":{"effort":"supported","supported_efforts":["low","low","medium","low"]}}]}"#;
        let summary = parse(body);
        let overlay = summary
            .capability_overlay
            .for_model("m")
            .expect("the contract is retained");
        let expected = labels(&["low", "medium"]);
        assert_eq!(
            overlay.record().accepted_effort_labels.as_deref(),
            Some(expected.as_slice())
        );
        assert!(overlay.record().validate().is_empty());
    }

    #[test]
    fn a_malformed_budget_range_degrades_the_budget_dimension_to_unknown() {
        let body = r#"{"data":[{"id":"m","name":"m","thinking":{"toggle":"unsupported","budget":"supported","budget_tokens_min":2000,"budget_tokens_max":1000}}]}"#;
        let summary = parse(body);
        let overlay = summary
            .capability_overlay
            .for_model("m")
            .expect("the contract is retained");
        let record = overlay.record();
        assert_eq!(record.budget_status, ReasoningCapabilityStatus::Unknown);
        assert!(record.min_thinking_budget.is_none());
        assert!(record.max_thinking_budget.is_none());
        // The dimension that was published soundly still stands.
        assert_eq!(
            record.disable_status,
            ReasoningCapabilityStatus::Unsupported
        );
    }

    #[test]
    fn an_oversized_budget_bound_degrades_the_budget_dimension_to_unknown() {
        let body = format!(
            "{{\"data\":[{{\"id\":\"m\",\"name\":\"m\",\"thinking\":{{\"toggle\":\"unsupported\",\"budget\":\"supported\",\"budget_tokens_max\":{}}}}}]}}",
            MAX_OVERLAY_THINKING_BUDGET_TOKENS + 1
        );
        let summary = parse(&body);
        let overlay = summary
            .capability_overlay
            .for_model("m")
            .expect("the contract is retained");
        assert_eq!(
            overlay.record().budget_status,
            ReasoningCapabilityStatus::Unknown
        );
        assert!(overlay.record().max_thinking_budget.is_none());
    }

    #[test]
    fn a_malformed_contract_does_not_fail_the_catalog_probe() {
        // The ladder is the wrong type, the status is nonsense, and the
        // budget bound is not a number: every dimension degrades, the
        // soundly published one still stands, and the probe still succeeds.
        let body = r#"{"data":[{"id":"m","name":"m","thinking":{"toggle":"unsupported","effort":"maybe","supported_efforts":[1,2,3],"budget_tokens_max":"lots"}}]}"#;
        let summary = parse(body);
        assert_eq!(summary.models.len(), 1);
        let overlay = summary
            .capability_overlay
            .for_model("m")
            .expect("the contract is retained");
        let record = overlay.record();
        assert_eq!(record.effort_status, ReasoningCapabilityStatus::Unknown);
        assert!(record.accepted_effort_labels.is_none());
        assert_eq!(record.budget_status, ReasoningCapabilityStatus::Unknown);
        assert!(record.max_thinking_budget.is_none());
        assert_eq!(
            record.disable_status,
            ReasoningCapabilityStatus::Unsupported
        );
    }

    #[test]
    fn a_contract_for_one_model_does_not_leak_into_another_models_record() {
        let body = r#"{"data":[{"id":"a","name":"a","thinking":{"effort":"supported","supported_efforts":["low"]}},{"id":"b","name":"b"}]}"#;
        let summary = parse(body);
        assert!(summary.capability_overlay.for_model("a").is_some());
        assert!(
            summary.capability_overlay.for_model("b").is_none(),
            "a contract for one model must not leak into another model's record"
        );
    }

    #[test]
    fn two_different_contracts_for_one_id_retain_no_overlay() {
        let body = r#"{"data":[{"id":"m","name":"m","thinking":{"effort":"supported","supported_efforts":["low"]}},{"id":"m","name":"m","thinking":{"effort":"supported","supported_efforts":["high"]}}]}"#;
        let summary = parse(body);
        assert_eq!(summary.models.len(), 1);
        assert!(
            summary.capability_overlay.for_model("m").is_none(),
            "two different contracts for one id are ambiguous, so neither is retained"
        );
    }

    #[test]
    fn the_always_reasons_claim_is_retained_but_never_reaches_the_record() {
        let body = r#"{"data":[{"id":"m","name":"m","thinking":{"always_reasons":true,"effort":"supported","supported_efforts":["low"]}}]}"#;
        let summary = parse(body);
        let overlay = summary
            .capability_overlay
            .for_model("m")
            .expect("the contract is retained");
        assert_eq!(
            overlay.always_reasons_claim(),
            Some(true),
            "the provider's claim is retained for reporting"
        );
        assert_eq!(
            overlay.record().always_reasons,
            None,
            "the compiled baseline owns the always-reasons property"
        );
        assert_eq!(
            overlay.record().always_reasons_status,
            ReasoningCapabilityStatus::Unknown
        );
    }

    #[test]
    fn the_overlay_is_replaced_atomically() {
        let slot = EggpoolCapabilityOverlaySlot::new();
        let first = parse(&response_with_ladder("m", &["low"])).capability_overlay;
        let second = parse(&response_with_ladder("m", &["medium"])).capability_overlay;

        slot.replace(first.clone());
        let held = slot.load();
        slot.replace(second.clone());

        assert_eq!(
            held.capability_revision(),
            first.capability_revision(),
            "a reader that already holds a snapshot keeps seeing the whole previous overlay"
        );
        assert_eq!(
            held.for_model("m")
                .and_then(|overlay| overlay.record().accepted_effort_labels.clone()),
            Some(labels(&["low"]))
        );
        assert_eq!(
            slot.load().capability_revision(),
            second.capability_revision(),
            "a new reader sees the whole new overlay"
        );
    }

    #[test]
    fn a_reader_never_observes_a_partially_replaced_overlay() {
        let slot = Arc::new(EggpoolCapabilityOverlaySlot::new());
        let first = parse(&response_with_ladder("m", &["low"])).capability_overlay;
        let second = parse(&response_with_ladder("m", &["medium"])).capability_overlay;
        slot.replace(first.clone());

        let stop = Arc::new(AtomicBool::new(false));
        let reader_slot = Arc::clone(&slot);
        let reader_stop = Arc::clone(&stop);
        let reader = thread::spawn(move || {
            let mut observed = Vec::new();
            while !reader_stop.load(Ordering::Acquire) {
                let snapshot = reader_slot.load();
                // Every observed snapshot is a whole overlay: the model is
                // always present and its ladder is always one of the two
                // published ones, never a mixture of the two.
                let ladder = snapshot
                    .for_model("m")
                    .expect("the model is always present")
                    .record()
                    .accepted_effort_labels
                    .clone()
                    .expect("a published ladder");
                observed.push(ladder);
            }
            observed
        });

        for _ in 0..500 {
            slot.replace(first.clone());
            slot.replace(second.clone());
        }
        stop.store(true, Ordering::Release);
        let observed = reader.join().expect("the reader finishes");

        let low = labels(&["low"]);
        let medium = labels(&["medium"]);
        for ladder in observed {
            assert!(
                ladder == low || ladder == medium,
                "a reader observed a partially replaced overlay: {ladder:?}"
            );
        }
    }

    #[test]
    fn a_direct_provider_connection_has_no_overlay() {
        let options = EggpoolProbeOptions::default();
        let with_contract = br#"{"data":[{"id":"m","name":"m","thinking":{"effort":"supported","supported_efforts":["low"]}}]}"#;
        let without_contract = br#"{"data":[{"id":"m","name":"m"}]}"#;
        // The direct-provider parser projects the response to models only:
        // it has nowhere to carry an overlay, so a published contract
        // changes nothing about what a direct provider connection sees.
        assert_eq!(
            parse_compatible_models_response(with_contract, &options).expect("the response parses"),
            parse_compatible_models_response(without_contract, &options)
                .expect("the response parses"),
        );
    }
}

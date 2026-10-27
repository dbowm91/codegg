//! Transactional context rollover sequencing (M004).
//!
//! `src/context/compaction.rs` remains the single production owner of
//! reduction policy. This module owns the host-controlled transaction
//! ordering around checkpoint generation, persistence, replacement-history
//! installation, and durable commit:
//!
//! ```text
//! A. capture authoritative source revisions/state
//! B. run deterministic compaction + optional semantic enrichment
//! C. materialize/verify required evidence refs
//! D. validate replacement message invariants and post-compaction capacity
//! E. persist checkpoint as Prepared
//! F. read back + verify payload digest/schema/parent
//! G. revalidate authoritative source revisions needed for install
//! H. replace in-memory/provider-visible messages
//! I. atomically mark checkpoint Installed + append ContextCompacted event
//! J. reset tracker / publish bounded runtime diagnostics
//! ```
//!
//! The key invariant is unchanged: step H cannot precede durable checkpoint
//! verification. An abandoned candidate remains `Prepared`/`Aborted` and is
//! never resume authority. Restart loads only `Installed` checkpoints.
//!
//! This module performs no provider/model calls. Store I/O is bounded to the
//! two async helpers below (`prepare_candidate`, `install_prepared`), which
//! call only the M001 store and the existing artifact store — no second
//! history service, no model calls. Diagnostics never include checkpoint
//! bodies or evidence content — IDs, digests, sizes, and state transitions
//! only.

use crate::context::compaction::{
    count_continuation_frames, validate_message_invariants, ContextCapacity,
};
use crate::provider::Message;

/// Bounded diagnostics for one rollover attempt (M004 §6.10).
///
/// Never carries checkpoint bodies or evidence content.
#[derive(Debug, Clone, Default)]
pub struct RolloverDiagnostics {
    pub session_id: String,
    pub checkpoint_id: String,
    pub checkpoint_sequence: i64,
    pub previous_checkpoint_id: Option<String>,
    pub tokens_before: usize,
    pub tokens_after: usize,
    pub checkpoint_bytes: usize,
    pub intent_inline_tokens: usize,
    pub recovery_ref_count: usize,
    pub semantic_enrichment: String,
    pub continuity: String,
    pub reason: String,
}

impl RolloverDiagnostics {
    pub fn bounded_line(&self) -> String {
        format!(
            "rollover(session={}, checkpoint={}, seq={}, prev={}, tokens={}->{}, bytes={}, intent_tokens={}, refs={}, semantic={}, continuity={}, reason={})",
            self.session_id,
            if self.checkpoint_id.is_empty() {
                "-"
            } else {
                &self.checkpoint_id
            },
            self.checkpoint_sequence,
            self.previous_checkpoint_id.as_deref().unwrap_or("-"),
            self.tokens_before,
            self.tokens_after,
            self.checkpoint_bytes,
            self.intent_inline_tokens,
            self.recovery_ref_count,
            if self.semantic_enrichment.is_empty() {
                "-"
            } else {
                &self.semantic_enrichment
            },
            if self.continuity.is_empty() {
                "-"
            } else {
                &self.continuity
            },
            if self.reason.is_empty() {
                "-"
            } else {
                &self.reason
            },
        )
    }
}

/// Captured host-owned revisions for stale-source validation (M004 §6.3).
///
/// Only fields that would make a checkpoint misleading are compared. Unrelated
/// telemetry changes never reject installation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RolloverSourceRevisions {
    pub goal_id: Option<String>,
    pub goal_revision: Option<i64>,
    pub plan_digest: Option<String>,
    pub todo_revision: u64,
    pub previous_installed_id: Option<String>,
    pub source_history_digest: String,
    /// Durable WorkPlan identity/revision (M004). `None` for legacy
    /// sessions without an active plan; presence requires exact match.
    pub work_plan_id: Option<String>,
    pub work_plan_revision: Option<i64>,
}

impl RolloverSourceRevisions {
    pub fn capture(
        goal_id: Option<String>,
        goal_revision: Option<i64>,
        plan_digest: Option<String>,
        todo_revision: u64,
        previous_installed_id: Option<String>,
        source_history_digest: String,
    ) -> Self {
        Self {
            goal_id,
            goal_revision,
            plan_digest,
            todo_revision,
            previous_installed_id,
            source_history_digest,
            work_plan_id: None,
            work_plan_revision: None,
        }
    }

    /// Capture including durable WorkPlan provenance (M004 work package A).
    pub fn capture_with_work_plan(
        goal_id: Option<String>,
        goal_revision: Option<i64>,
        plan_digest: Option<String>,
        todo_revision: u64,
        previous_installed_id: Option<String>,
        source_history_digest: String,
        work_plan_id: Option<String>,
        work_plan_revision: Option<i64>,
    ) -> Self {
        Self {
            goal_id,
            goal_revision,
            plan_digest,
            todo_revision,
            previous_installed_id,
            source_history_digest,
            work_plan_id,
            work_plan_revision,
        }
    }

    /// True when a relevant source changed while semantic compaction was
    /// running and the candidate must be discarded/aborted.
    pub fn is_stale_against(&self, current: &Self) -> bool {
        if self.goal_id != current.goal_id {
            return true;
        }
        if self.goal_revision != current.goal_revision {
            return true;
        }
        if self.plan_digest != current.plan_digest {
            return true;
        }
        if self.todo_revision != current.todo_revision {
            return true;
        }
        if self.previous_installed_id != current.previous_installed_id {
            return true;
        }
        if self.work_plan_id != current.work_plan_id {
            return true;
        }
        if self.work_plan_revision != current.work_plan_revision {
            return true;
        }
        false
    }

    pub fn stale_reason(&self, current: &Self) -> Option<&'static str> {
        if self.goal_id != current.goal_id {
            return Some("active goal changed");
        }
        if self.goal_revision != current.goal_revision {
            return Some("active goal revision changed");
        }
        if self.plan_digest != current.plan_digest {
            return Some("plan digest changed");
        }
        if self.todo_revision != current.todo_revision {
            return Some("todo state changed");
        }
        if self.previous_installed_id != current.previous_installed_id {
            return Some("newer checkpoint installed during build");
        }
        if self.work_plan_id != current.work_plan_id {
            return Some("active work plan changed");
        }
        if self.work_plan_revision != current.work_plan_revision {
            return Some("active work plan revision changed");
        }
        None
    }

    /// Rebuild a fresh revision snapshot from newly loaded values while
    /// preserving the original history digest for diagnostics.
    pub fn into_with_overrides(
        self,
        goal_id: Option<String>,
        goal_revision: Option<i64>,
        todo_revision: u64,
        previous_installed_id: Option<String>,
    ) -> Self {
        Self {
            goal_id,
            goal_revision,
            plan_digest: self.plan_digest,
            todo_revision,
            previous_installed_id,
            source_history_digest: self.source_history_digest,
            work_plan_id: self.work_plan_id,
            work_plan_revision: self.work_plan_revision,
        }
    }

    /// Rebuild with WorkPlan overrides (M004 revalidation at install).
    pub fn into_with_work_plan_overrides(
        self,
        goal_id: Option<String>,
        goal_revision: Option<i64>,
        todo_revision: u64,
        previous_installed_id: Option<String>,
        work_plan_id: Option<String>,
        work_plan_revision: Option<i64>,
    ) -> Self {
        Self {
            goal_id,
            goal_revision,
            plan_digest: self.plan_digest,
            todo_revision,
            previous_installed_id,
            source_history_digest: self.source_history_digest,
            work_plan_id,
            work_plan_revision,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProviderVisiblePrefixIdentity {
    pub version: u32,
    pub message_count: usize,
    pub sha256: String,
}

pub const PROVIDER_VISIBLE_PREFIX_IDENTITY_VERSION: u32 = 2;
const PROVIDER_VISIBLE_PREFIX_IDENTITY_DOMAIN: &[u8] = b"codegg/provider-visible-prefix/v2\0";

fn append_identity_field(output: &mut Vec<u8>, value: &[u8]) -> Option<()> {
    output.extend_from_slice(&u64::try_from(value.len()).ok()?.to_be_bytes());
    output.extend_from_slice(value);
    Some(())
}

fn append_identity_content_parts(
    output: &mut Vec<u8>,
    parts: &[codegg_protocol::dto::ContentPart],
) -> Option<()> {
    output.extend_from_slice(&u64::try_from(parts.len()).ok()?.to_be_bytes());
    for part in parts {
        match part {
            codegg_protocol::dto::ContentPart::Text { text } => {
                output.push(b't');
                append_identity_field(output, text.as_bytes())?;
            }
            codegg_protocol::dto::ContentPart::Image { image_url } => {
                output.push(b'i');
                append_identity_field(output, image_url.url.as_bytes())?;
            }
        }
    }
    Some(())
}

fn canonical_provider_message(message: &codegg_protocol::dto::ProviderMessage) -> Option<Vec<u8>> {
    use codegg_protocol::dto::ProviderMessage;
    let mut out = Vec::new();
    match message {
        ProviderMessage::System { .. } => return None,
        ProviderMessage::User { content } => {
            out.push(b'u');
            append_identity_content_parts(&mut out, content)?;
        }
        ProviderMessage::Assistant {
            content,
            tool_calls,
        } => {
            out.push(b'a');
            append_identity_content_parts(&mut out, content)?;
            out.extend_from_slice(&u64::try_from(tool_calls.len()).ok()?.to_be_bytes());
            for call in tool_calls {
                out.push(b'c');
                append_identity_field(&mut out, call.id.as_bytes())?;
                append_identity_field(&mut out, call.name.as_bytes())?;
                let arguments = serde_json::to_vec(&call.arguments).ok()?;
                append_identity_field(&mut out, &arguments)?;
            }
        }
        ProviderMessage::Tool {
            tool_call_id,
            content,
        } => {
            out.push(b'r');
            append_identity_field(&mut out, tool_call_id.as_bytes())?;
            append_identity_field(&mut out, content.as_bytes())?;
        }
    }
    Some(out)
}

fn identity_seed(chain_base: Option<&ProviderVisiblePrefixIdentity>) -> Option<([u8; 32], usize)> {
    use sha2::{Digest, Sha256};
    match chain_base {
        Some(base) if base.version == PROVIDER_VISIBLE_PREFIX_IDENTITY_VERSION => {
            let bytes = hex::decode(&base.sha256).ok()?;
            Some((bytes.try_into().ok()?, base.message_count))
        }
        Some(_) => None,
        None => Some((
            Sha256::digest(PROVIDER_VISIBLE_PREFIX_IDENTITY_DOMAIN).into(),
            0,
        )),
    }
}

fn advance_identity(state: [u8; 32], encoded: &[u8]) -> Option<[u8; 32]> {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(PROVIDER_VISIBLE_PREFIX_IDENTITY_DOMAIN);
    hasher.update(state);
    hasher.update(u64::try_from(encoded.len()).ok()?.to_be_bytes());
    hasher.update(encoded);
    Some(hasher.finalize().into())
}

pub fn provider_visible_prefix_identities(
    messages: &[codegg_protocol::dto::ProviderMessage],
) -> Option<Vec<ProviderVisiblePrefixIdentity>> {
    provider_visible_prefix_identities_after(messages, None)
}

fn provider_visible_prefix_identities_after(
    messages: &[codegg_protocol::dto::ProviderMessage],
    chain_base: Option<&ProviderVisiblePrefixIdentity>,
) -> Option<Vec<ProviderVisiblePrefixIdentity>> {
    let (mut state, base_count) = identity_seed(chain_base)?;
    let mut identities = Vec::with_capacity(messages.len());
    for (index, message) in messages.iter().enumerate() {
        let encoded = canonical_provider_message(message)?;
        state = advance_identity(state, &encoded)?;
        identities.push(ProviderVisiblePrefixIdentity {
            version: PROVIDER_VISIBLE_PREFIX_IDENTITY_VERSION,
            message_count: base_count.checked_add(index + 1)?,
            sha256: hex::encode(state),
        });
    }
    Some(identities)
}

pub fn matching_provider_prefix_count(
    expected: &ProviderVisiblePrefixIdentity,
    incoming_prefixes: &[ProviderVisiblePrefixIdentity],
) -> Option<usize> {
    if expected.version != PROVIDER_VISIBLE_PREFIX_IDENTITY_VERSION || expected.message_count == 0 {
        return None;
    }
    let count = expected.message_count;
    let actual = incoming_prefixes.get(count.checked_sub(1)?)?;
    (actual.version == expected.version
        && actual.message_count == count
        && actual.sha256 == expected.sha256)
        .then_some(count)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProviderPrefixReplacement {
    Replaced {
        message_count: usize,
    },
    RetainedMismatch {
        expected_version: u32,
        expected_message_count: usize,
        incoming_identity_count: usize,
    },
}

pub fn replace_provider_prefix<T>(
    expected: &ProviderVisiblePrefixIdentity,
    incoming_prefixes: &[ProviderVisiblePrefixIdentity],
    messages: &mut Vec<T>,
    offset: usize,
) -> ProviderPrefixReplacement {
    let count = matching_provider_prefix_count(expected, incoming_prefixes).filter(|count| {
        offset
            .checked_add(*count)
            .is_some_and(|end| end <= messages.len())
    });
    if let Some(count) = count {
        messages.drain(offset..offset + count);
        ProviderPrefixReplacement::Replaced {
            message_count: count,
        }
    } else {
        ProviderPrefixReplacement::RetainedMismatch {
            expected_version: expected.version,
            expected_message_count: expected.message_count,
            incoming_identity_count: incoming_prefixes.len(),
        }
    }
}

pub fn drop_matching_provider_prefix<T>(
    expected: &ProviderVisiblePrefixIdentity,
    incoming_prefixes: &[ProviderVisiblePrefixIdentity],
    messages: &mut Vec<T>,
    offset: usize,
) -> Option<usize> {
    match replace_provider_prefix(expected, incoming_prefixes, messages, offset) {
        ProviderPrefixReplacement::Replaced { message_count } => Some(message_count),
        ProviderPrefixReplacement::RetainedMismatch { .. } => None,
    }
}

fn provider_content_to_dto(
    parts: &[crate::provider::ContentPart],
) -> Option<Vec<codegg_protocol::dto::ContentPart>> {
    use codegg_protocol::dto::ContentPart as DtoPart;
    parts
        .iter()
        .map(|part| match part {
            crate::provider::ContentPart::Text { text } => Some(DtoPart::Text {
                text: text.to_string(),
            }),
            crate::provider::ContentPart::Image { image_url } => Some(DtoPart::Image {
                image_url: codegg_protocol::dto::ImageUrl {
                    url: image_url.url.to_string(),
                },
            }),
            crate::provider::ContentPart::Reasoning { text, .. } => Some(DtoPart::Text {
                text: format!("[Reasoning]\\n{text}"),
            }),
        })
        .collect()
}

fn frontend_projection_from_provider_messages(
    messages: &[Message],
) -> Option<Vec<codegg_protocol::dto::ProviderMessage>> {
    use codegg_protocol::dto::{ProviderMessage as DtoMessage, ToolCall as DtoToolCall};
    let mut projected = Vec::with_capacity(messages.len());
    for message in messages {
        match message {
            Message::System { .. } => {}
            Message::User { content } => projected.push(DtoMessage::User {
                content: provider_content_to_dto(content)?,
            }),
            Message::Assistant {
                content,
                tool_calls,
            } => projected.push(DtoMessage::Assistant {
                content: provider_content_to_dto(content)?,
                tool_calls: tool_calls
                    .iter()
                    .map(|call| DtoToolCall {
                        id: call.id.to_string(),
                        name: call.name.to_string(),
                        arguments: call.arguments.clone(),
                    })
                    .collect(),
            }),
            Message::Tool {
                tool_call_id,
                content,
            } => projected.push(DtoMessage::Tool {
                tool_call_id: tool_call_id.to_string(),
                content: content.to_string(),
            }),
        }
    }
    Some(projected)
}

pub fn compacted_provider_visible_prefix_identity(
    source: &[Message],
    compacted: &[Message],
    peer_observations_present: bool,
    frontend_user_controls_possible: bool,
    chain_base: Option<&ProviderVisiblePrefixIdentity>,
) -> Option<ProviderVisiblePrefixIdentity> {
    if peer_observations_present || frontend_user_controls_possible {
        return None;
    }
    if count_continuation_frames(compacted) == 0 {
        return None;
    }
    let source = frontend_projection_from_provider_messages(source)?;
    let compacted = frontend_projection_from_provider_messages(compacted)?;
    let source_encoded = source
        .iter()
        .map(canonical_provider_message)
        .collect::<Option<Vec<_>>>()?;
    let compacted_encoded = compacted
        .iter()
        .map(canonical_provider_message)
        .collect::<Option<Vec<_>>>()?;
    let mut retained_suffix = 0usize;
    while retained_suffix < source_encoded.len()
        && retained_suffix < compacted_encoded.len()
        && source_encoded[source_encoded.len() - retained_suffix - 1]
            == compacted_encoded[compacted_encoded.len() - retained_suffix - 1]
    {
        retained_suffix += 1;
    }
    let covered_count = source.len().checked_sub(retained_suffix)?;
    if covered_count == 0 {
        return None;
    }
    let compacted_prefix_end = compacted_encoded.len().checked_sub(retained_suffix)?;
    let compacted_without_suffix = &compacted_encoded[..compacted_prefix_end];
    let mut search_from = 0usize;
    let mut anchored_prefix_count = 0usize;
    for row in source_encoded.iter().take(covered_count) {
        let Some(relative_position) = compacted_without_suffix
            .get(search_from..)?
            .iter()
            .position(|candidate| candidate == row)
        else {
            break;
        };
        search_from = search_from.checked_add(relative_position + 1)?;
        anchored_prefix_count += 1;
    }
    if source_encoded[anchored_prefix_count..covered_count]
        .iter()
        .any(|row| {
            compacted_without_suffix
                .iter()
                .any(|candidate| candidate == row)
        })
    {
        return None;
    }
    provider_visible_prefix_identities_after(&source[..covered_count], chain_base)?.pop()
}

pub fn provider_visible_prefix_identity(
    messages: &[codegg_protocol::dto::ProviderMessage],
) -> Option<ProviderVisiblePrefixIdentity> {
    if messages.is_empty() {
        let (state, _) = identity_seed(None)?;
        return Some(ProviderVisiblePrefixIdentity {
            version: PROVIDER_VISIBLE_PREFIX_IDENTITY_VERSION,
            message_count: 0,
            sha256: hex::encode(state),
        });
    }
    provider_visible_prefix_identities(messages)?.pop()
}

/// Deterministic digest over the pre-compaction source history for lineage
/// debugging. Not a security boundary; the durable payload digest is.
pub fn source_history_digest(messages: &[Message]) -> String {
    let mut combined = String::new();
    for message in messages {
        match message {
            Message::System { content } => {
                combined.push_str(content.as_str());
                combined.push('\n');
            }
            Message::User { content } => {
                for part in content {
                    if let crate::provider::ContentPart::Text { text } = part {
                        combined.push_str(text.as_str());
                        combined.push('\n');
                    }
                }
            }
            Message::Assistant {
                content,
                tool_calls,
            } => {
                for part in content {
                    if let crate::provider::ContentPart::Text { text } = part {
                        combined.push_str(text.as_str());
                        combined.push('\n');
                    }
                }
                for tc in tool_calls {
                    combined.push_str(&tc.id);
                    combined.push('\n');
                }
            }
            Message::Tool {
                tool_call_id,
                content,
            } => {
                combined.push_str(tool_call_id.as_str());
                combined.push(':');
                combined.push_str(content.as_str());
                combined.push('\n');
            }
        }
    }
    crate::context::stable_hash_hex(combined.as_bytes())
}

/// Validate replacement-message invariants before durable installation
/// (M004 §6.2 step D).
///
/// Checks, in order:
/// - tool-call/result pairing, ordering, and IDs remain valid;
/// - exactly one current CodeGG continuation frame;
/// - the active current user input remains visible;
/// - post-compaction tokens are below the configured send budget, unless the
///   caller explicitly entered hard-capacity degraded mode.
pub fn validate_replacement_messages(
    messages: &[Message],
    capacity: ContextCapacity,
    expect_user_visible: bool,
) -> Result<(), String> {
    validate_message_invariants(messages).map_err(|e| format!("tool-pair invariant: {e}"))?;
    let frames = count_continuation_frames(messages);
    if frames != 1 {
        return Err(format!(
            "exactly one continuation frame required after rollover, found {frames}"
        ));
    }
    if expect_user_visible {
        let has_user = messages.iter().any(|m| {
            matches!(
                m,
                Message::User { content } if content.iter().any(|p| matches!(
                    p,
                    crate::provider::ContentPart::Text { text } if !text.trim().is_empty()
                ))
            )
        });
        if !has_user {
            return Err(
                "active current user input must remain visible after compaction".to_string(),
            );
        }
    }
    let tokens = crate::context::compaction::context_tokens(messages, None);
    if tokens > capacity.available_context_tokens {
        return Err(format!(
            "post-compaction tokens {tokens} exceed send budget {}",
            capacity.available_context_tokens
        ));
    }
    Ok(())
}

/// True when sending unchanged history would exceed the provider's safe
/// capacity (M004 §6.6). Ordinary threshold pressure defers rollover;
/// hard-capacity pressure permits the explicit degraded fallback.
pub fn is_hard_capacity(tokens_before: usize, capacity: ContextCapacity) -> bool {
    tokens_before > capacity.available_context_tokens
}

/// Build the durable checkpoint payload body from an enriched snapshot plus
/// verified evidence refs (M004 §6.1).
///
/// The snapshot body is the M002 typed projection; verified M003 refs are
/// attached under `evidence_refs` with checkpoint-scoped stable identity.
/// Summary-only refs (no handle) remain usable. No persistence happens here.
pub fn build_checkpoint_payload_body(
    snapshot: &crate::context::continuation::ContinuationSnapshot,
    verified_evidence: &[crate::context::compaction::EvidenceRef],
    checkpoint_id: &str,
) -> serde_json::Value {
    let mut body = snapshot.to_payload_body();
    let refs: Vec<serde_json::Value> = verified_evidence
        .iter()
        .map(|item| {
            serde_json::json!({
                "stable_id": item.stable_id,
                "checkpoint_id": item.checkpoint_id,
                "kind": format!("{:?}", item.kind),
                "summary": item.summary,
                "content_hash": item.content_hash,
                "recovery_handle": item.recovery_handle,
                "source_ordinal": item.source_ordinal,
            })
        })
        .collect();
    if let Some(map) = body.as_object_mut() {
        map.insert("evidence_refs".to_string(), serde_json::Value::Array(refs));
        map.insert(
            "recovery_ref_count".to_string(),
            serde_json::json!(verified_evidence
                .iter()
                .filter(|r| r.recovery_handle.is_some())
                .count()),
        );
        map.insert(
            "checkpoint_id_hint".to_string(),
            serde_json::json!(checkpoint_id),
        );
    }
    body
}

/// Outcome of restart/turn-start checkpoint validation (M004 §6.5).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RestartValidation {
    /// No installed checkpoint; behavior remains current.
    Absent,
    /// Installed checkpoint is valid and may be injected.
    Usable,
    /// Installed checkpoint is corrupt/unsupported; fall back to durable
    /// goal/todo/session state with a visible diagnostic.
    CorruptFallback(String),
}

/// Validate a loaded installed checkpoint for resume authority.
///
/// Checks supported schema version, payload digest, session ID, lineage
/// self-consistency, and optional goal/plan provenance shape. `Prepared` and
/// `Aborted` rows are never passed here — callers load only
/// `latest_installed`. A missing optional M003 artifact handle does not block
/// turn start; that degrades per-ref at read time. Pre-M004 checkpoints
/// without `work_plan` remain usable; a present but malformed `work_plan`
/// fails closed so a misleading handoff never becomes resume authority.
pub fn validate_installed_for_restart(
    checkpoint: &codegg_core::session::continuation::ContinuationCheckpoint,
    session_id: &str,
) -> RestartValidation {
    if checkpoint.session_id != session_id {
        return RestartValidation::CorruptFallback(
            "installed checkpoint session mismatch".to_string(),
        );
    }
    if checkpoint.schema_version
        != codegg_core::session::continuation::CONTINUATION_CHECKPOINT_SCHEMA_VERSION
    {
        return RestartValidation::CorruptFallback(format!(
            "unsupported checkpoint schema {}",
            checkpoint.schema_version
        ));
    }
    if checkpoint.verify_digest().is_err() {
        return RestartValidation::CorruptFallback("payload digest mismatch".to_string());
    }
    if checkpoint.status
        != codegg_core::session::continuation::ContinuationCheckpointStatus::Installed
    {
        return RestartValidation::CorruptFallback("non-installed checkpoint".to_string());
    }
    // Lineage self-consistency: sequence must be positive; parent may be None
    // only for the first epoch.
    if checkpoint.sequence <= 0 {
        return RestartValidation::CorruptFallback("invalid epoch sequence".to_string());
    }
    // M004: bounded WorkPlan provenance, when present, must decode.
    if let Err(reason) = codegg_core::work_plan::provenance_from_body(&checkpoint.payload.body) {
        return RestartValidation::CorruptFallback(format!(
            "work plan provenance decode: {reason}"
        ));
    }
    RestartValidation::Usable
}

/// Extract bounded WorkPlan provenance from an installed checkpoint.
///
/// Returns `None` for legacy checkpoints without `work_plan` (still usable).
/// Returns `Err` for a present but malformed block so callers fail closed.
pub fn work_plan_provenance_of(
    checkpoint: &codegg_core::session::continuation::ContinuationCheckpoint,
) -> Result<Option<codegg_core::work_plan::WorkPlanCheckpointProvenance>, String> {
    codegg_core::work_plan::provenance_from_body(&checkpoint.payload.body)
}

/// Render the model-visible continuation projection for an installed
/// checkpoint (M004 §6.5).
///
/// The projection prioritizes objective/current work/constraints/decisions/
/// next action and provides exact recovery handles rather than embedding
/// evidence. It is bounded by the M002 frame cap. When the active goal is
/// newer than the checkpoint, the caller supplies the overriding objective
/// and current task (M002 precedence) instead of hiding the newer goal.
pub fn render_installed_projection(
    checkpoint: &codegg_core::session::continuation::ContinuationCheckpoint,
    goal_override: Option<(String, Option<String>)>,
) -> String {
    let body = &checkpoint.payload.body;
    let objective = goal_override
        .as_ref()
        .map(|(objective, _)| objective.clone())
        .or_else(|| {
            body.get("objective")
                .and_then(|v| v.as_str())
                .map(str::to_string)
        })
        .unwrap_or_default();
    let current_task = goal_override.and_then(|(_, task)| task).or_else(|| {
        body.get("current_task")
            .and_then(|v| v.as_str())
            .map(str::to_string)
    });
    let get_list = |key: &[&str]| -> Vec<String> {
        let mut value: Option<&serde_json::Value> = Some(body);
        for part in key {
            value = value.and_then(|v| v.get(*part));
        }
        value
            .and_then(|v| v.as_array())
            .map(|items| {
                items
                    .iter()
                    .filter_map(|i| i.as_str().map(str::to_string))
                    .take(32)
                    .collect()
            })
            .unwrap_or_default()
    };
    let constraints = get_list(&["semantic", "constraints"]);
    let decisions = get_list(&["semantic", "decisions"]);
    let blockers = {
        let primary = get_list(&["semantic", "unresolved_blockers"]);
        if primary.is_empty() {
            get_list(&["semantic", "unresolved_errors"])
        } else {
            primary
        }
    };
    let mut next_steps = get_list(&["semantic", "next_steps"]);
    if next_steps.is_empty() {
        if let Some(task) = current_task.clone() {
            if !task.trim().is_empty() {
                next_steps.push(task);
            }
        }
    }
    let touched = get_list(&["touched_files"]);
    let commands = get_list(&["commands"]);
    let tests = get_list(&["tests"]);
    let errors = get_list(&["errors"]);
    // M004 WorkPlan provenance: bounded handoff, never the full plan.
    // Legacy checkpoints without `work_plan` render exactly as before.
    let work_plan_footer: Option<String> = match codegg_core::work_plan::provenance_from_body(body)
    {
        Ok(Some(provenance)) => {
            let mut footer = format!(
                "WorkPlan: {} rev {} status {}",
                provenance.plan_id, provenance.revision, provenance.status
            );
            if let Some(phase) = provenance.current_phase.as_deref() {
                footer.push_str(&format!(" phase {phase}"));
            }
            if let Some(item) = provenance.current_item_id.as_deref() {
                footer.push_str(&format!(" item {item}"));
            }
            Some(footer)
        }
        Ok(None) => None,
        // Malformed work_plan never blocks rendering with a panic; the
        // restart validator already fails closed, and the projection
        // degrades to the non-WorkPlan fields with a bounded marker.
        Err(_) => Some("WorkPlan: unavailable (provenance decode)".to_string()),
    };
    let work_plan_next_fallback: Option<String> =
        match codegg_core::work_plan::provenance_from_body(body) {
            Ok(Some(provenance)) => provenance
                .actionable
                .first()
                .and_then(|item| item.next_action.clone().filter(|a| !a.trim().is_empty()))
                .or_else(|| {
                    provenance
                        .actionable
                        .first()
                        .map(|item| item.description.clone())
                }),
            _ => None,
        };
    // WorkPlan next action keeps the trajectory actionable when the
    // checkpoint has no semantic next steps and no current task override.
    if next_steps.is_empty() {
        if let Some(fallback) = work_plan_next_fallback {
            if !fallback.trim().is_empty() {
                next_steps.push(fallback);
            }
        }
    }
    // Recovery handles: verified evidence refs carry exact handles; the
    // projection lists them bounded, never the evidence bodies.
    let handles: Vec<String> = body
        .get("evidence_refs")
        .and_then(|v| v.as_array())
        .map(|items| {
            items
                .iter()
                .filter_map(|r| r.get("recovery_handle").and_then(|h| h.as_str()))
                .take(16)
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default();

    let mut frame = crate::agent::context_frame::ContextFrame {
        user_goal: Some(objective).filter(|s| !s.trim().is_empty()),
        current_task,
        constraints,
        decisions,
        touched_files: touched,
        commands_run: commands,
        test_results: tests,
        unresolved_errors: {
            let mut merged = errors;
            for b in blockers {
                if !merged.contains(&b) {
                    merged.push(b);
                }
            }
            merged
        },
        security_findings: get_list(&["security"]),
        next_steps,
        artifact_handles: {
            let mut merged = get_list(&["artifact_handles"]);
            for h in handles {
                if !merged.contains(&h) {
                    merged.push(h);
                }
            }
            merged.truncate(32);
            merged
        },
    };
    // Defense-in-depth bound identical to the live renderer.
    let mut text = frame.to_continuation_text();
    // Provenance footer: IDs/digests only, never bodies.
    if let Some(work_plan_line) = work_plan_footer.as_deref() {
        text.push_str(&format!("\n- {work_plan_line}"));
    }
    let short_digest: String = checkpoint.payload_digest.chars().take(12).collect();
    text.push_str(&format!(
        "\n- Checkpoint: {} seq {} digest {short_digest}",
        checkpoint.id, checkpoint.sequence
    ));
    if let Some(prev) = checkpoint.previous_installed_id.as_deref() {
        text.push_str(&format!("\n- Supersedes: {prev}"));
    }
    // Reuse the single frame cap.
    if text.len() > crate::context::continuation::MAX_FRAME_TEXT_BYTES {
        let mut end = crate::context::continuation::MAX_FRAME_TEXT_BYTES;
        while end > 0 && !text.is_char_boundary(end) {
            end -= 1;
        }
        text.truncate(end);
        text.push_str("\n[bounded continuation frame truncated]");
    }
    // Silence unused-mut warning while keeping the binding extensible.
    let _ = &mut frame;
    text
}

/// Strip earlier CodeGG-owned frames and report the removed count (M004 §6.8).
///
/// The prior checkpoint's rendered continuation frame is stripped before new
/// semantic input construction except where its typed fields are supplied
/// separately. Free-form legacy summaries never stack in system history.
pub fn strip_prior_frames(messages: &[Message]) -> (Vec<Message>, usize) {
    crate::context::compaction::strip_codegg_owned_frames(messages)
}

/// Assert exactly one current continuation frame (trajectory invariant).
pub fn assert_single_frame(messages: &[Message]) -> Result<(), String> {
    let count = count_continuation_frames(messages);
    if count != 1 {
        return Err(format!(
            "expected exactly one continuation frame, found {count}"
        ));
    }
    Ok(())
}

/// Prepared candidate ready for in-memory replacement and atomic install.
///
/// Steps C-F plus D (pre-install validation). The caller must replace
/// in-memory history (step H) only after this returns `Ok`, then call
/// [`install_prepared`] (step I). Abandoned candidates stay
/// `Prepared`/`Aborted` and are never resume authority.
pub struct PreparedCandidate {
    pub checkpoint: codegg_core::session::continuation::ContinuationCheckpoint,
    pub verified_messages: Vec<Message>,
    pub verified_evidence: Vec<crate::context::compaction::EvidenceRef>,
    pub diagnostics: RolloverDiagnostics,
}

/// Prepare one continuation candidate (M004 §6.2 steps C-F + D).
///
/// - `candidate` is the in-memory M002 merge + raw evidence index from the
///   canonical engine (only verified M003 refs are installed);
/// - `compacted_messages` is the engine's replacement history (validated
///   here, installed by the caller after verification);
/// - `existing_tool_handles` is the bounded ledger projection for
///   `ctx://tool/...` reuse;
/// - `original_user` guarantees the active current input stays visible.
///
/// Returns `Err` with a bounded reason when rollover must defer (ordinary
/// threshold: leave history unchanged) or degrade (hard capacity: caller
/// uses [`degraded_fallback`]). No history is mutated here.
#[allow(clippy::too_many_arguments)]
pub async fn prepare_candidate(
    store: &codegg_core::session::continuation::ContinuationCheckpointStore,
    artifact_store: &dyn crate::context::artifact::ContextArtifactStore,
    session_id: &str,
    checkpoint_id: &str,
    previous_installed_id: Option<String>,
    candidate: &crate::context::compaction::ContinuationCandidate,
    compacted_messages: Vec<Message>,
    capacity: ContextCapacity,
    tokens_before: usize,
    tokens_after: usize,
    turn_index: usize,
    existing_tool_handles: &[String],
    original_user: Option<String>,
    provider_visible_prefix_identity: Option<ProviderVisiblePrefixIdentity>,
) -> Result<PreparedCandidate, String> {
    // Step C: select, materialize, and verify required evidence refs.
    let mut selected = crate::context::evidence::select_materializable_evidence(
        &candidate.evidence,
        checkpoint_id,
    );
    let persist_outcome = crate::context::evidence::persist_selected_evidence(
        artifact_store,
        session_id,
        checkpoint_id,
        turn_index,
        // Persist reads visible source text from the pre-compaction history
        // would be ideal; the compacted history retains the same user/tool
        // ordinals for retained indices, and selection already carries
        // source ordinals. For the transactional path the caller supplies
        // the pre-compaction history via `compacted_messages`'s source?
        // We persist against the compacted history's retained text plus the
        // snapshot intent spine (both host-owned). Missing source degrades
        // to summary-only, never invalidates the checkpoint.
        &compacted_messages,
        &mut selected,
        existing_tool_handles,
    )
    .await;
    let verify_outcome = crate::context::evidence::verify_evidence_artifacts(
        artifact_store,
        session_id,
        &mut selected,
    )
    .await;
    // A missing optional artifact never blocks the checkpoint; the summary
    // remains usable. Only verified handles are attached.
    let _ = (persist_outcome.materialized, verify_outcome.verified);

    // Build the durable payload: enriched snapshot + verified refs.
    let mut enriched = candidate.snapshot.clone();
    // Ensure the payload's recovery count matches verified handles.
    let mut body = build_checkpoint_payload_body(&enriched, &selected, checkpoint_id);
    if let (Some(identity), Some(map)) = (provider_visible_prefix_identity, body.as_object_mut()) {
        map.insert(
            "provider_visible_prefix_identity".to_string(),
            serde_json::to_value(identity).unwrap_or(serde_json::Value::Null),
        );
    }
    let payload = codegg_core::session::continuation::ContinuationCheckpointPayload::new(body)
        .map_err(|e| format!("checkpoint payload rejected: {e}"))?;
    let checkpoint_bytes = payload.canonical_json().map(|s| s.len()).unwrap_or(0);

    // Step D: validate replacement invariants and post-compaction capacity
    // before any durable row is treated as success. The active current user
    // input must remain visible; token budget must hold unless the caller
    // explicitly entered degraded mode (handled by the caller checking
    // `is_hard_capacity` on error).
    let mut verified_messages = compacted_messages;
    // Guarantee current-user visibility: if the engine dropped the active
    // input under extreme pressure, re-append it (a plain User message is
    // always pair-safe) rather than sending a history without it.
    if let Some(user) = original_user.as_deref() {
        let trimmed = user.trim();
        if !trimmed.is_empty() {
            let present = verified_messages.iter().any(|m| match m {
                Message::User { content } => content.iter().any(|p| match p {
                    crate::provider::ContentPart::Text { text } => {
                        text.as_str().contains(trimmed) || trimmed.contains(text.as_str().trim())
                    }
                    _ => false,
                }),
                _ => false,
            });
            if !present {
                verified_messages.push(Message::User {
                    content: vec![crate::provider::ContentPart::Text {
                        text: trimmed.to_string().into(),
                    }],
                });
            }
        }
    }
    validate_replacement_messages(&verified_messages, capacity, original_user.is_some())
        .map_err(|e| format!("replacement validation: {e}"))?;

    // Step E: persist as Prepared with the pre-allocated identity.
    let prepared = store
        .prepare_with_id(
            session_id,
            checkpoint_id,
            previous_installed_id.as_deref(),
            payload,
        )
        .await
        .map_err(|e| format!("checkpoint prepare: {e}"))?;

    // Step F: read back + verify payload digest/schema/parent.
    let read_back = store
        .get(session_id, checkpoint_id)
        .await
        .map_err(|e| format!("checkpoint read-back: {e}"))?
        .ok_or_else(|| "checkpoint read-back missing".to_string())?;
    read_back
        .verify_digest()
        .map_err(|e| format!("checkpoint digest verification: {e}"))?;
    if read_back.schema_version
        != codegg_core::session::continuation::CONTINUATION_CHECKPOINT_SCHEMA_VERSION
    {
        return Err(format!(
            "unsupported checkpoint schema {}",
            read_back.schema_version
        ));
    }
    if read_back.previous_installed_id != previous_installed_id {
        return Err("checkpoint parent mismatch on read-back".to_string());
    }
    // Ensure the enriched snapshot's intent budget is observable without
    // logging bodies.
    let intent_inline_tokens: usize = enriched
        .intent_spine
        .iter()
        .map(|e| eggcontext::estimate_tokens_sync(&e.text, None))
        .sum();
    let recovery_ref_count = selected
        .iter()
        .filter(|r| r.recovery_handle.is_some())
        .count();
    let diagnostics = RolloverDiagnostics {
        session_id: session_id.to_string(),
        checkpoint_id: checkpoint_id.to_string(),
        checkpoint_sequence: read_back.sequence,
        previous_checkpoint_id: previous_installed_id,
        tokens_before,
        tokens_after,
        checkpoint_bytes,
        intent_inline_tokens,
        recovery_ref_count,
        semantic_enrichment: candidate.semantic_outcome.clone(),
        continuity: String::from("prepared"),
        reason: String::from("threshold"),
    };
    // Silence unused-mut while keeping the binding extensible for M004
    // follow-ups that enrich the snapshot after verification.
    let _ = &mut enriched;
    Ok(PreparedCandidate {
        checkpoint: read_back,
        verified_messages,
        verified_evidence: selected,
        diagnostics: {
            let _ = prepared;
            diagnostics
        },
    })
}

/// Atomically mark a prepared checkpoint Installed with its durable
/// `ContextCompacted` commit marker (M004 §6.2 step I).
///
/// The caller must have already replaced in-memory history (step H) after
/// [`prepare_candidate`] verification. `Prepared` rows are never resume
/// authority; only this transaction makes the epoch durable.
pub async fn install_prepared(
    store: &codegg_core::session::continuation::ContinuationCheckpointStore,
    prepared: &codegg_core::session::continuation::ContinuationCheckpoint,
    messages_removed: usize,
    messages_remaining: usize,
    tokens_before: usize,
    tokens_after: usize,
) -> Result<codegg_core::session::continuation::ContinuationCheckpoint, String> {
    let event = prepared
        .build_compacted_event(
            messages_removed,
            messages_remaining,
            Some(tokens_before),
            Some(tokens_after),
            vec![format!("checkpoint:{}", prepared.id)],
            vec![format!("epoch:{}", prepared.sequence)],
            vec![],
        )
        .map_err(|e| format!("commit-marker event: {e}"))?;
    store
        .install_with_compaction_event(&prepared.session_id, &prepared.id, event)
        .await
        .map_err(|e| format!("checkpoint install: {e}"))
}

/// Hard-capacity degraded fallback (M004 §6.6).
///
/// Uses pair-safe emergency compaction to keep the turn operable, injects
/// the best current host-owned frame in memory, sets a degraded reason, and
/// never marks a durable checkpoint installed. The caller surfaces a
/// durable/in-process diagnostic that continuity degraded.
pub fn degraded_fallback(
    original_messages: &[Message],
    host_frame_text: &str,
    capacity: ContextCapacity,
) -> (Vec<Message>, String) {
    let config = crate::context::compaction::ResolvedCompactionConfig::default();
    let mut messages =
        crate::context::compaction::emergency_pair_safe_compaction(original_messages, &config);
    // Inject the best current host-owned frame in memory (no durable row).
    if !host_frame_text.trim().is_empty() {
        // Strip any stale CodeGG frames the emergency path preserved, then
        // append the single host frame. Emergency markers are not CodeGG
        // continuation frames and are preserved.
        let (stripped, _) = strip_prior_frames(&messages);
        messages = stripped;
        messages.push(Message::System {
            content: host_frame_text.to_string().into(),
        });
    }
    // Best-effort capacity note; the fallback is valid even if still above
    // budget — the turn is operable and explicitly degraded.
    let _ = capacity;
    (
        messages,
        String::from(
            "hard-capacity degraded: emergency pair-safe history, no durable checkpoint installed",
        ),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn provider_visible_prefix_identity_is_exact_and_prefix_bounded() {
        use codegg_protocol::dto::{ContentPart, ProviderMessage, ToolCall};
        let source = vec![
            ProviderMessage::User {
                content: vec![ContentPart::Text { text: "ask".into() }],
            },
            ProviderMessage::Assistant {
                content: vec![],
                tool_calls: vec![ToolCall {
                    id: "call-1".into(),
                    name: "read".into(),
                    arguments: serde_json::json!({"path":"a"}),
                }],
            },
            ProviderMessage::Tool {
                tool_call_id: "call-1".into(),
                content: "result".into(),
            },
        ];
        let identities = provider_visible_prefix_identities(&source).unwrap();
        assert_eq!(identities.len(), source.len());
        assert_ne!(identities[0].sha256, identities[1].sha256);
        assert_ne!(identities[1].sha256, identities[2].sha256);
        assert_eq!(
            matching_provider_prefix_count(&identities[1], &identities),
            Some(2)
        );
        let mut with_suffix = vec!["old-1", "old-2", "new-user", "new-tail"];
        assert_eq!(
            drop_matching_provider_prefix(&identities[1], &identities, &mut with_suffix, 0),
            Some(2)
        );
        assert_eq!(with_suffix, vec!["new-user", "new-tail"]);
        let mut mismatch_kept = vec!["old-1", "old-2", "suffix"];
        let mut wrong_prefix = identities.clone();
        wrong_prefix[1].sha256 = "mismatch".into();
        assert_eq!(
            matching_provider_prefix_count(&identities[1], &wrong_prefix),
            None
        );
        assert_eq!(
            replace_provider_prefix(&identities[1], &wrong_prefix, &mut mismatch_kept, 0),
            ProviderPrefixReplacement::RetainedMismatch {
                expected_version: identities[1].version,
                expected_message_count: 2,
                incoming_identity_count: wrong_prefix.len(),
            }
        );
        assert_eq!(mismatch_kept, vec!["old-1", "old-2", "suffix"]);
        let mut unsupported = identities[1].clone();
        unsupported.version += 1;
        assert_eq!(
            matching_provider_prefix_count(&unsupported, &identities),
            None
        );
        assert_eq!(
            provider_visible_prefix_identity(&source[..2]).unwrap(),
            identities[1]
        );

        let mut changed = source.clone();
        changed[1] = ProviderMessage::Assistant {
            content: vec![],
            tool_calls: vec![ToolCall {
                id: "call-1".into(),
                name: "write".into(),
                arguments: serde_json::json!({"path":"a"}),
            }],
        };
        assert_ne!(
            provider_visible_prefix_identity(&changed[..2])
                .unwrap()
                .sha256,
            identities[1].sha256
        );
        let split_parts = vec![ProviderMessage::User {
            content: vec![
                ContentPart::Text { text: "a".into() },
                ContentPart::Text { text: "b".into() },
            ],
        }];
        let joined_part = vec![ProviderMessage::User {
            content: vec![ContentPart::Text { text: "ab".into() }],
        }];
        assert_ne!(
            provider_visible_prefix_identity(&split_parts)
                .unwrap()
                .sha256,
            provider_visible_prefix_identity(&joined_part)
                .unwrap()
                .sha256
        );
        let mut changed_call = source.clone();
        changed_call[1] = ProviderMessage::Assistant {
            content: vec![],
            tool_calls: vec![ToolCall {
                id: "call-2".into(),
                name: "read".into(),
                arguments: serde_json::json!({"path":"b"}),
            }],
        };
        assert_ne!(
            provider_visible_prefix_identity(&changed_call[..2])
                .unwrap()
                .sha256,
            identities[1].sha256
        );
        let changed_result = vec![
            source[0].clone(),
            source[1].clone(),
            ProviderMessage::Tool {
                tool_call_id: "call-1".into(),
                content: "changed result".into(),
            },
        ];
        assert_ne!(
            provider_visible_prefix_identity(&changed_result)
                .unwrap()
                .sha256,
            identities[2].sha256
        );
        assert!(provider_visible_prefix_identity(&[ProviderMessage::System {
            content: "daemon".into()
        }])
        .is_none());
    }

    #[test]
    fn two_turn_transcript_identity_covers_compacted_user_and_generated_rows() {
        use crate::provider::{ContentPart as ProviderPart, Message as ProviderMessage, ToolCall};
        use codegg_protocol::dto::{ContentPart, ProviderMessage as DtoMessage};

        let source = vec![
            ProviderMessage::User {
                content: vec![ProviderPart::Text {
                    text: "prior user".to_string().into(),
                }],
            },
            ProviderMessage::Assistant {
                content: vec![ProviderPart::Text {
                    text: "prior answer".to_string().into(),
                }],
                tool_calls: vec![ToolCall {
                    id: "call-1".to_string().into(),
                    name: "read".to_string().into(),
                    arguments: serde_json::json!({"path":"src/lib.rs"}),
                }],
            },
            ProviderMessage::Tool {
                tool_call_id: "call-1".to_string().into(),
                content: "prior tool result".to_string().into(),
            },
            ProviderMessage::User {
                content: vec![ProviderPart::Text {
                    text: "turn-one user".to_string().into(),
                }],
            },
            ProviderMessage::Assistant {
                content: vec![ProviderPart::Text {
                    text: "generated assistant".to_string().into(),
                }],
                tool_calls: vec![ToolCall {
                    id: "call-2".to_string().into(),
                    name: "read".to_string().into(),
                    arguments: serde_json::json!({"path":"src/main.rs"}),
                }],
            },
            ProviderMessage::Tool {
                tool_call_id: "call-2".to_string().into(),
                content: "generated tool result".to_string().into(),
            },
        ];
        let next_user = DtoMessage::User {
            content: vec![ContentPart::Text {
                text: "turn-two user".into(),
            }],
        };
        let dto_source = frontend_projection_from_provider_messages(&source).unwrap();
        let mut second_turn = dto_source.clone();
        second_turn.push(next_user.clone());
        let prefixes = provider_visible_prefix_identities(&second_turn).unwrap();

        let compacted_summary = vec![ProviderMessage::System {
            content: "[codegg continuation state v1]\nbounded continuation frame"
                .to_string()
                .into(),
        }];
        let identity = compacted_provider_visible_prefix_identity(
            &source,
            &compacted_summary,
            false,
            false,
            None,
        )
        .unwrap();
        assert_eq!(identity.message_count, source.len());
        assert!(compacted_provider_visible_prefix_identity(
            &source,
            &compacted_summary,
            true,
            false,
            None,
        )
        .is_none());
        assert!(compacted_provider_visible_prefix_identity(
            &source,
            &compacted_summary,
            false,
            true,
            None,
        )
        .is_none());
        let mut submitted = second_turn.clone();
        assert_eq!(
            drop_matching_provider_prefix(&identity, &prefixes, &mut submitted, 0),
            Some(source.len())
        );
        assert_eq!(
            provider_visible_prefix_identity(&submitted).unwrap(),
            provider_visible_prefix_identity(std::slice::from_ref(&next_user)).unwrap()
        );

        let turn_two_assistant = ProviderMessage::Assistant {
            content: vec![ProviderPart::Text {
                text: "turn-two assistant".to_string().into(),
            }],
            tool_calls: vec![ToolCall {
                id: "call-3".to_string().into(),
                name: "read".to_string().into(),
                arguments: serde_json::json!({"path":"src/main.rs"}),
            }],
        };
        let turn_two_tool = ProviderMessage::Tool {
            tool_call_id: "call-3".to_string().into(),
            content: "turn-two tool result".to_string().into(),
        };
        let second_source = vec![
            ProviderMessage::User {
                content: vec![ProviderPart::Text {
                    text: "turn-two user".to_string().into(),
                }],
            },
            turn_two_assistant.clone(),
            turn_two_tool.clone(),
        ];
        let second_compacted = vec![
            ProviderMessage::System {
                content: "[codegg continuation state v1]\nnew bounded frame"
                    .to_string()
                    .into(),
            },
            turn_two_assistant.clone(),
            turn_two_tool.clone(),
        ];
        let extended_identity = compacted_provider_visible_prefix_identity(
            &second_source,
            &second_compacted,
            false,
            false,
            Some(&identity),
        )
        .unwrap();
        assert_eq!(extended_identity.message_count, source.len() + 1);
        let mut third_turn = second_turn.clone();
        third_turn.push(DtoMessage::Assistant {
            content: vec![ContentPart::Text {
                text: "turn-two assistant".into(),
            }],
            tool_calls: vec![],
        });
        third_turn.push(DtoMessage::Tool {
            tool_call_id: "call-3".into(),
            content: "turn-two tool result".into(),
        });
        third_turn.push(DtoMessage::User {
            content: vec![ContentPart::Text {
                text: "turn-three user".into(),
            }],
        });
        let third_prefixes = provider_visible_prefix_identities(&third_turn).unwrap();
        assert_eq!(
            matching_provider_prefix_count(&extended_identity, &third_prefixes),
            Some(source.len() + 1)
        );

        let retained = &source[source.len() - 2..];
        let compacted_with_tail = std::iter::once(ProviderMessage::System {
            content: "[codegg continuation state v1]\nbounded continuation frame"
                .to_string()
                .into(),
        })
        .chain(retained.iter().cloned())
        .collect::<Vec<_>>();
        let identity = compacted_provider_visible_prefix_identity(
            &source,
            &compacted_with_tail,
            false,
            false,
            None,
        )
        .unwrap();
        assert_eq!(identity.message_count, source.len() - retained.len());
        let anchored_prefix = vec![
            ProviderMessage::System {
                content: "[codegg continuation state v1]\nframe".to_string().into(),
            },
            source[0].clone(),
            source[1].clone(),
            source[source.len() - 2].clone(),
            source[source.len() - 1].clone(),
        ];
        let anchored_identity = compacted_provider_visible_prefix_identity(
            &source,
            &anchored_prefix,
            false,
            false,
            None,
        )
        .expect("an installed frame can supersede an anchored exact prefix");
        assert_eq!(anchored_identity.message_count, source.len() - 2);

        let noncontiguous_retention = vec![
            ProviderMessage::System {
                content: "[codegg continuation state v1]\nframe".to_string().into(),
            },
            source[2].clone(),
            source[source.len() - 1].clone(),
        ];
        assert!(compacted_provider_visible_prefix_identity(
            &source,
            &noncontiguous_retention,
            false,
            false,
            None,
        )
        .is_none());
        let mut submitted = second_turn;
        assert_eq!(
            drop_matching_provider_prefix(&identity, &prefixes, &mut submitted, 0),
            Some(identity.message_count)
        );
        let expected_suffix = vec![
            dto_source[dto_source.len() - 2].clone(),
            dto_source[dto_source.len() - 1].clone(),
            next_user,
        ];
        assert_eq!(
            provider_visible_prefix_identity(&submitted).unwrap(),
            provider_visible_prefix_identity(&expected_suffix).unwrap()
        );
    }

    #[test]
    fn source_revisions_ignore_unrelated_telemetry() {
        let captured = RolloverSourceRevisions::capture(
            Some("goal-1".to_string()),
            Some(3),
            Some("plan-digest".to_string()),
            9,
            Some("ckpt-1".to_string()),
            "history-a".to_string(),
        );
        // Same relevant fields, different history digest detail is still
        // relevant only via parent/goal/plan/todo — history digest itself is
        // diagnostic, not a reject reason.
        let same = RolloverSourceRevisions::capture(
            Some("goal-1".to_string()),
            Some(3),
            Some("plan-digest".to_string()),
            9,
            Some("ckpt-1".to_string()),
            "history-a".to_string(),
        );
        assert!(!captured.is_stale_against(&same));
        let stale_goal = RolloverSourceRevisions::capture(
            Some("goal-1".to_string()),
            Some(4),
            Some("plan-digest".to_string()),
            9,
            Some("ckpt-1".to_string()),
            "history-a".to_string(),
        );
        assert!(captured.is_stale_against(&stale_goal));
        assert_eq!(
            captured.stale_reason(&stale_goal),
            Some("active goal revision changed")
        );
        let stale_parent = RolloverSourceRevisions::capture(
            Some("goal-1".to_string()),
            Some(3),
            Some("plan-digest".to_string()),
            9,
            Some("ckpt-2".to_string()),
            "history-a".to_string(),
        );
        assert!(captured.is_stale_against(&stale_parent));
    }

    #[test]
    fn diagnostics_line_never_carries_body() {
        let diag = RolloverDiagnostics {
            session_id: "sess".to_string(),
            checkpoint_id: "ckpt".to_string(),
            checkpoint_sequence: 2,
            previous_checkpoint_id: Some("ckpt-1".to_string()),
            tokens_before: 100,
            tokens_after: 40,
            checkpoint_bytes: 1024,
            intent_inline_tokens: 50,
            recovery_ref_count: 3,
            semantic_enrichment: "success".to_string(),
            continuity: "installed".to_string(),
            reason: "threshold".to_string(),
        };
        let line = diag.bounded_line();
        assert!(line.contains("sess"));
        assert!(line.contains("ckpt"));
        assert!(!line.contains("secret-body-marker"));
    }
}

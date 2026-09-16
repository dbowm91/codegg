//! Bounded exact context recovery references (M003).
//!
//! Host-owned helper for materializing selected compaction evidence into
//! the existing durable [`FileArtifactStore`] and exposing stable
//! checkpoint-scoped recovery handles through `context_read`.
//!
//! Ownership: `src/context/compaction.rs` remains the single production
//! owner of reduction policy. This module is a bounded persistence helper
//! it (and M004) calls; it performs no provider/model calls, no semantic
//! search, and no cross-session lookup.
//!
//! Wire format:
//! `ctx://evidence/{session_id}/{checkpoint_id}/{evidence_id}`
//! where `evidence_id` is deterministically derived from source
//! ordinal/kind plus content digest, never a model-invented path.
//!
//! Bounds (documented, enforced before any artifact write):
//! - max 64 evidence refs per checkpoint;
//! - max 256 KiB total new evidence bytes per checkpoint;
//! - max 64 KiB per single continuation-evidence artifact;
//! - max 280 chars per model-visible ref summary.

use std::collections::{HashMap, HashSet};
use std::sync::LazyLock;

use regex::Regex;

use crate::context::artifact::{ArtifactKind, ContextArtifact, ContextArtifactStore};
use crate::context::compaction::{EvidenceKind, EvidenceRef};
use crate::context::handle::ContextHandle;
use crate::provider::{ContentPart, Message};

/// Maximum evidence refs carried per checkpoint (M003 §6.4).
pub const MAX_EVIDENCE_REFS_PER_CHECKPOINT: usize = 64;
/// Maximum total new evidence artifact bytes per checkpoint (M003 §6.4).
pub const MAX_EVIDENCE_TOTAL_BYTES: usize = 256 * 1024;
/// Maximum single continuation-evidence artifact bytes (M003 §6.4).
pub const MAX_SINGLE_EVIDENCE_BYTES: usize = 64 * 1024;
/// Maximum model-visible summary chars per ref (M003 §6.4).
pub const MAX_EVIDENCE_SUMMARY_CHARS: usize = 280;

static SECRET_ASSIGNMENT_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r#"(?i)(api[_-]?key|secret|password|passwd|pwd|bearer|authorization|credential|credentials|private[_-]?key|client[_-]?secret|access[_-]?token)\s*[:=]\s*("[^"]+"|'[^']+'|`[^`]+`|\S+)"#,
    )
    .expect("valid secret assignment regex")
});

static SECRET_TOKEN_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(sk-[A-Za-z0-9_-]{8,}|ghp_[A-Za-z0-9_]{8,}|github_pat_[A-Za-z0-9_]{8,}|xox[bap]-[A-Za-z0-9-]+|AKIA[A-Z0-9]{16}|eyJ[A-Za-z0-9_-]{8,})",
    )
    .expect("valid secret token regex")
});

/// Deterministic priority for checkpoint evidence selection (M003 §6.4).
///
/// Lower wins. `ToolCall` is never materialized as new evidence because it
/// carries raw tool-argument JSON; it returns `u8::MAX` so selection skips
/// it. All other kinds are bounded and redactable.
pub fn evidence_priority(kind: &EvidenceKind) -> u8 {
    match kind {
        EvidenceKind::UserMessage => 0,
        EvidenceKind::TestRun => 1,
        EvidenceKind::ToolResult => 2,
        EvidenceKind::AssistantMessage => 3,
        EvidenceKind::Command => 4,
        EvidenceKind::Diff => 4,
        EvidenceKind::FilePath => 4,
        EvidenceKind::SecurityFinding => 4,
        EvidenceKind::Todo => 5,
        EvidenceKind::ToolCall => u8::MAX,
    }
}

fn evidence_kind_slug(kind: &EvidenceKind) -> &'static str {
    match kind {
        EvidenceKind::UserMessage => "user",
        EvidenceKind::AssistantMessage => "assistant",
        EvidenceKind::ToolCall => "toolcall",
        EvidenceKind::ToolResult => "tool",
        EvidenceKind::TestRun => "test",
        EvidenceKind::FilePath => "file",
        EvidenceKind::Command => "cmd",
        EvidenceKind::Diff => "diff",
        EvidenceKind::SecurityFinding => "sec",
        EvidenceKind::Todo => "todo",
    }
}

fn truncate_chars(text: &str, max_chars: usize) -> String {
    if text.chars().count() <= max_chars {
        return text.to_string();
    }
    let truncated: String = text.chars().take(max_chars).collect();
    format!("{truncated}…[truncated]")
}

fn truncate_bytes_utf8_safe(text: &str, max_bytes: usize) -> &str {
    if text.len() <= max_bytes {
        return text;
    }
    let mut end = max_bytes;
    while end > 0 && !text.is_char_boundary(end) {
        end -= 1;
    }
    &text[..end]
}

/// Redact secret-bearing material from evidence text (M003 §6.5).
///
/// - Applies Git URL-credential redaction (reuses current policy);
/// - replaces `key = value` / `key: value` secret assignments with
///   `[redacted]`;
/// - replaces standalone secret tokens (`sk-…`, `ghp_…`, …) with
///   `[redacted]`.
///
/// The returned digest must always be computed over the redacted body so
/// read verification is meaningful.
pub fn redact_evidence_text(text: &str) -> String {
    let redacted_urls = crate::git_network_policy::redact_url_credentials_in_text(text);
    let redacted_assignments = SECRET_ASSIGNMENT_RE.replace_all(&redacted_urls, "$1=[redacted]");
    SECRET_TOKEN_RE
        .replace_all(&redacted_assignments, "[redacted]")
        .into_owned()
}

/// Extract visible text for evidence materialization (M003 §6.5).
///
/// Persists `User`/`Assistant` visible `Text` only. Provider-private
/// `Reasoning` and `Image` parts are never persisted. Tool-call argument
/// JSON is never persisted as conversation evidence (callers must skip
/// `EvidenceKind::ToolCall`).
fn visible_text_for_materialization(
    messages: &[Message],
    source_ordinal: Option<usize>,
) -> Option<String> {
    let index = source_ordinal?;
    let message = messages.get(index)?;
    match message {
        Message::User { content } => {
            let text = content
                .iter()
                .filter_map(|part| match part {
                    ContentPart::Text { text } => Some(text.as_str()),
                    _ => None,
                })
                .collect::<Vec<_>>()
                .join(" ");
            let trimmed = text.trim();
            (!trimmed.is_empty()).then(|| trimmed.to_string())
        }
        Message::Assistant { content, .. } => {
            let text = content
                .iter()
                .filter_map(|part| match part {
                    ContentPart::Text { text } => Some(text.as_str()),
                    _ => None,
                })
                .collect::<Vec<_>>()
                .join(" ");
            let trimmed = text.trim();
            (!trimmed.is_empty()).then(|| trimmed.to_string())
        }
        Message::Tool { content, .. } => {
            let trimmed = content.trim();
            (!trimmed.is_empty()).then(|| trimmed.to_string())
        }
        Message::System { .. } => None,
    }
}

/// Deterministic checkpoint-scoped evidence ID.
///
/// Derived from source ordinal, kind slug, and content digest prefix — never
/// from model-invented paths. Safe as a `ctx://evidence/...` segment (no
/// `/`, whitespace, or control characters).
pub fn deterministic_evidence_id(
    kind: &EvidenceKind,
    source_ordinal: usize,
    content_digest: &str,
) -> String {
    let digest_prefix: String = content_digest.chars().take(12).collect();
    let digest_safe: String = digest_prefix
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { 'x' })
        .collect();
    format!(
        "ev-{}-{:04}-{}",
        evidence_kind_slug(kind),
        source_ordinal.min(9999),
        digest_safe
    )
}

/// Deterministic checkpoint-scoped stable identity for an installed ref.
///
/// Pass-local IDs (`msg_0001`, `tool_0001`) remain local diagnostics only;
/// installed refs carry `stable_id = "{checkpoint_id}:{evidence_id}"` so the
/// identity stays stable across later compactions and restart.
pub fn stable_id_for(checkpoint_id: &str, evidence_id: &str) -> String {
    format!("{checkpoint_id}:{evidence_id}")
}

/// Select bounded materializable evidence for one checkpoint candidate.
///
/// - Skips `ToolCall` (raw argument JSON is never evidence);
/// - assigns `checkpoint_id`, deterministic `stable_id`, and bounded
///   summaries;
/// - orders by priority (user steering → failures → tests → decisions →
///   recent assistant) with deterministic recency tie-break;
/// - caps to [`MAX_EVIDENCE_REFS_PER_CHECKPOINT`].
///
/// The returned refs have `recovery_handle = None`; callers persist them
/// with [`persist_selected_evidence`] then verify with
/// [`verify_evidence_artifacts`] before attaching to a checkpoint payload
/// (M003 §6.7 ordering for M004).
pub fn select_materializable_evidence(
    evidence: &[EvidenceRef],
    checkpoint_id: &str,
) -> Vec<EvidenceRef> {
    let mut candidates: Vec<(u8, i64, EvidenceRef)> = Vec::new();
    for item in evidence {
        if matches!(item.kind, EvidenceKind::ToolCall) {
            continue;
        }
        let priority = evidence_priority(&item.kind);
        if priority == u8::MAX {
            continue;
        }
        let ordinal = item.source_ordinal.unwrap_or(usize::MAX);
        let digest = item.content_hash.clone().unwrap_or_default();
        let evidence_id = deterministic_evidence_id(&item.kind, ordinal.min(9999), &digest);
        let stable_id = stable_id_for(checkpoint_id, &evidence_id);
        let summary = truncate_chars(
            &redact_evidence_text(&item.summary),
            MAX_EVIDENCE_SUMMARY_CHARS,
        );
        // Recency tie-break: most recent source ordinal first when
        // priorities tie, so the cap keeps high-value recent evidence.
        // Use negative ordinal for descending sort via i64.
        let recency = -(ordinal.min(i64::MAX as usize) as i64);
        candidates.push((
            priority,
            recency,
            EvidenceRef {
                id: item.id.clone(),
                kind: item.kind.clone(),
                summary,
                content_hash: item.content_hash.clone(),
                recovery_handle: None,
                stable_id: Some(stable_id),
                checkpoint_id: Some(checkpoint_id.to_string()),
                source_ordinal: item.source_ordinal,
                tool_call_id: item.tool_call_id.clone(),
            },
        ));
    }
    candidates.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.cmp(&b.1)));
    candidates.truncate(MAX_EVIDENCE_REFS_PER_CHECKPOINT);
    // Emit in deterministic source order so checkpoint payloads hash
    // stably for equivalent input.
    candidates.sort_by_key(|(_, _, item)| item.source_ordinal.unwrap_or(usize::MAX));
    candidates.into_iter().map(|(_, _, item)| item).collect()
}

/// Outcome of [`persist_selected_evidence`].
#[derive(Debug, Default)]
pub struct PersistOutcome {
    pub materialized: usize,
    pub reused: usize,
    pub summary_only: usize,
    pub bytes_written: usize,
    pub diagnostics: Vec<String>,
}

/// Persist selected evidence with a candidate checkpoint ID (M003 §6.7).
///
/// For each selected ref, in order:
/// 1. prefer an existing `ctx://tool/...` handle when the detail already
///    lives in the artifact store (no duplicate write);
/// 2. otherwise write one bounded redacted `ContinuationEvidence` artifact
///    keyed by `ctx://evidence/{session}/{checkpoint}/{evidence_id}`;
/// 3. on any write failure, leave the ref summary-only (no handle) with a
///    bounded diagnostic — never invalidate the whole checkpoint.
///
/// `existing_tool_handles` is the bounded ledger projection (e.g.
/// `ContextLedgerState::artifact_handles`). `turn_index` supplies the
/// required artifact `turn_index` field for new evidence artifacts.
///
/// Enforces [`MAX_SINGLE_EVIDENCE_BYTES`] per artifact and
/// [`MAX_EVIDENCE_TOTAL_BYTES`] across the checkpoint.
pub async fn persist_selected_evidence(
    store: &dyn ContextArtifactStore,
    session_id: &str,
    checkpoint_id: &str,
    turn_index: usize,
    messages: &[Message],
    selected: &mut [EvidenceRef],
    existing_tool_handles: &[String],
) -> PersistOutcome {
    let mut outcome = PersistOutcome::default();
    // Index existing tool handles by tool_call_id for exact reuse.
    let mut tool_handle_by_call: HashMap<String, String> = HashMap::new();
    for handle in existing_tool_handles {
        if let Ok(parsed) = ContextHandle::parse(handle) {
            if !parsed.same_session(session_id) {
                continue;
            }
            if let (Some(call_id), true) = (parsed.tool_call_id(), parsed.is_tool()) {
                tool_handle_by_call
                    .entry(call_id.to_string())
                    .or_insert_with(|| handle.clone());
            }
        }
    }
    // De-duplicate identical recovery targets within one checkpoint.
    let mut seen_handles: HashSet<String> = HashSet::new();

    for item in selected.iter_mut() {
        // Never persist tool-call argument JSON.
        if matches!(item.kind, EvidenceKind::ToolCall) {
            item.recovery_handle = None;
            outcome.summary_only += 1;
            outcome.diagnostics.push(format!(
                "evidence({}): tool_call skipped, summary only",
                item.id
            ));
            continue;
        }
        // Prefer existing tool artifact handles (M003 §6.3).
        if let Some(call_id) = item.tool_call_id.clone() {
            if let Some(existing) = tool_handle_by_call.get(&call_id) {
                if seen_handles.insert(existing.clone()) {
                    match store.get(existing).await {
                        Ok(Some(artifact)) if artifact.session_id == session_id => {
                            item.recovery_handle = Some(existing.clone());
                            outcome.reused += 1;
                            continue;
                        }
                        Ok(_) => {
                            // Missing or cross-session artifact: fall
                            // through to bounded new write when source
                            // text is available, else summary-only.
                        }
                        Err(error) => {
                            outcome.diagnostics.push(format!(
                                "evidence({}): existing handle read failed: {error}",
                                item.id
                            ));
                        }
                    }
                } else {
                    // Duplicate source within one checkpoint: reuse the
                    // already-attached handle.
                    item.recovery_handle = Some(existing.clone());
                    outcome.reused += 1;
                    continue;
                }
            }
        }
        // New bounded evidence artifact from visible source text.
        let Some(source_text) = visible_text_for_materialization(messages, item.source_ordinal)
        else {
            item.recovery_handle = None;
            outcome.summary_only += 1;
            outcome.diagnostics.push(format!(
                "evidence({}): no source text, summary only",
                item.id
            ));
            continue;
        };
        let redacted = redact_evidence_text(&source_text);
        if redacted.trim().is_empty() {
            item.recovery_handle = None;
            outcome.summary_only += 1;
            outcome
                .diagnostics
                .push(format!("evidence({}): empty after redaction", item.id));
            continue;
        }
        let bounded = truncate_bytes_utf8_safe(&redacted, MAX_SINGLE_EVIDENCE_BYTES);
        if outcome.bytes_written + bounded.len() > MAX_EVIDENCE_TOTAL_BYTES {
            item.recovery_handle = None;
            outcome.summary_only += 1;
            outcome.diagnostics.push(format!(
                "evidence({}): total bytes cap reached, summary only",
                item.id
            ));
            continue;
        }
        let digest = crate::context::compute_content_hash(bounded);
        let ordinal = item.source_ordinal.unwrap_or(0);
        let evidence_id = deterministic_evidence_id(&item.kind, ordinal, &digest);
        let handle = match ContextHandle::build_evidence(session_id, checkpoint_id, &evidence_id) {
            Ok(handle) => handle,
            Err(error) => {
                item.recovery_handle = None;
                outcome.summary_only += 1;
                outcome
                    .diagnostics
                    .push(format!("evidence({}): bad handle: {error}", item.id));
                continue;
            }
        };
        if !seen_handles.insert(handle.clone()) {
            item.recovery_handle = Some(handle);
            outcome.reused += 1;
            continue;
        }
        let artifact = ContextArtifact {
            handle: handle.clone(),
            session_id: session_id.to_string(),
            turn_index,
            tool_call_id: None,
            tool_name: None,
            kind: ArtifactKind::ContinuationEvidence,
            created_at_ms: chrono::Utc::now().timestamp_millis(),
            content_hash: digest.clone(),
            redacted_content: bounded.to_string(),
            raw_bytes_len: bounded.len(),
            estimated_tokens: crate::context::estimate_tokens(bounded),
        };
        match store.put(artifact).await {
            Ok(()) => {
                item.recovery_handle = Some(handle);
                item.content_hash = Some(digest);
                item.checkpoint_id = Some(checkpoint_id.to_string());
                item.stable_id = Some(stable_id_for(checkpoint_id, &evidence_id));
                // Keep the model-visible summary consistent with the
                // redacted stored body.
                item.summary = truncate_chars(&redacted, MAX_EVIDENCE_SUMMARY_CHARS);
                outcome.materialized += 1;
                outcome.bytes_written += bounded.len();
            }
            Err(error) => {
                item.recovery_handle = None;
                outcome.summary_only += 1;
                outcome
                    .diagnostics
                    .push(format!("evidence({}): write failed: {error}", item.id));
            }
        }
    }
    outcome.diagnostics.push(format!(
        "evidence_persist(checkpoint={checkpoint_id}, materialized={}, reused={}, summary_only={}, bytes={})",
        outcome.materialized, outcome.reused, outcome.summary_only, outcome.bytes_written
    ));
    outcome
}

/// Outcome of [`verify_evidence_artifacts`].
#[derive(Debug, Default)]
pub struct VerifyOutcome {
    pub verified: usize,
    pub degraded: usize,
    pub diagnostics: Vec<String>,
}

/// Verify attached evidence handles before checkpoint installation.
///
/// Reads back every `recovery_handle` by exact handle, checks same-session
/// ownership and content-digest agreement, and degrades missing or
/// mismatched optional refs to summary-only (`recovery_handle = None`)
/// rather than invalidating the checkpoint (M003 §8, §6.8).
pub async fn verify_evidence_artifacts(
    store: &dyn ContextArtifactStore,
    session_id: &str,
    refs: &mut [EvidenceRef],
) -> VerifyOutcome {
    let mut outcome = VerifyOutcome::default();
    for item in refs.iter_mut() {
        let Some(handle) = item.recovery_handle.clone() else {
            outcome.degraded += 1;
            continue;
        };
        let parsed = match ContextHandle::parse(&handle) {
            Ok(parsed) => parsed,
            Err(error) => {
                item.recovery_handle = None;
                outcome.degraded += 1;
                outcome
                    .diagnostics
                    .push(format!("evidence({}): bad handle: {error}", item.id));
                continue;
            }
        };
        if !parsed.same_session(session_id) {
            item.recovery_handle = None;
            outcome.degraded += 1;
            outcome.diagnostics.push(format!(
                "evidence({}): cross-session handle rejected",
                item.id
            ));
            continue;
        }
        match store.get(&handle).await {
            Ok(Some(artifact)) => {
                if artifact.handle != handle || artifact.session_id != session_id {
                    item.recovery_handle = None;
                    outcome.degraded += 1;
                    outcome.diagnostics.push(format!(
                        "evidence({}): handle/session mismatch, summary only",
                        item.id
                    ));
                    continue;
                }
                if let Some(expected) = item.content_hash.as_deref() {
                    if artifact.content_hash != expected {
                        item.recovery_handle = None;
                        outcome.degraded += 1;
                        outcome.diagnostics.push(format!(
                            "evidence({}): digest mismatch, summary only",
                            item.id
                        ));
                        continue;
                    }
                }
                // Digest of the stored body must agree with its hash so
                // read verification is meaningful.
                let actual = crate::context::compute_content_hash(&artifact.redacted_content);
                if artifact.content_hash != actual {
                    item.recovery_handle = None;
                    outcome.degraded += 1;
                    outcome.diagnostics.push(format!(
                        "evidence({}): stored digest mismatch, summary only",
                        item.id
                    ));
                    continue;
                }
                outcome.verified += 1;
            }
            Ok(None) => {
                item.recovery_handle = None;
                outcome.degraded += 1;
                outcome.diagnostics.push(format!(
                    "evidence({}): missing artifact, summary only",
                    item.id
                ));
            }
            Err(error) => {
                item.recovery_handle = None;
                outcome.degraded += 1;
                outcome.diagnostics.push(format!(
                    "evidence({}): verify read failed: {error}",
                    item.id
                ));
            }
        }
    }
    outcome.diagnostics.push(format!(
        "evidence_verify(verified={}, degraded={})",
        outcome.verified, outcome.degraded
    ));
    outcome
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::context::InMemoryArtifactStore;
    use std::sync::Arc;

    fn user(text: &str) -> Message {
        Message::User {
            content: vec![ContentPart::Text {
                text: Arc::new(text.to_string()),
            }],
        }
    }

    fn assistant(text: &str) -> Message {
        Message::Assistant {
            content: vec![ContentPart::Text {
                text: Arc::new(text.to_string()),
            }],
            tool_calls: vec![],
        }
    }

    fn tool_result(call_id: &str, content: &str) -> Message {
        Message::Tool {
            tool_call_id: Arc::new(call_id.to_string()),
            content: Arc::new(content.to_string()),
        }
    }

    #[test]
    fn evidence_id_is_deterministic_and_safe() {
        let first = deterministic_evidence_id(&EvidenceKind::UserMessage, 3, "abcdef1234567890");
        let second = deterministic_evidence_id(&EvidenceKind::UserMessage, 3, "abcdef1234567890");
        assert_eq!(first, second);
        assert!(first.starts_with("ev-user-0003-"));
        assert!(!first.contains('/'));
        assert!(!first.contains(' '));
        let different =
            deterministic_evidence_id(&EvidenceKind::UserMessage, 4, "abcdef1234567890");
        assert_ne!(first, different);
    }

    #[test]
    fn stable_id_includes_checkpoint_scope() {
        let stable = stable_id_for("ckpt-1", "ev-user-0003-abc");
        assert!(stable.contains("ckpt-1"));
        assert!(stable.contains("ev-user-0003-abc"));
    }

    #[test]
    fn selection_skips_tool_calls_and_caps_refs() {
        let mut evidence = Vec::new();
        for index in 0..80 {
            evidence.push(EvidenceRef {
                id: format!("msg_{index:04}"),
                kind: EvidenceKind::UserMessage,
                summary: format!("steering {index}"),
                content_hash: Some(format!("hash{index:04}")),
                recovery_handle: None,
                stable_id: None,
                checkpoint_id: None,
                source_ordinal: Some(index),
                tool_call_id: None,
            });
        }
        evidence.push(EvidenceRef {
            id: "tool_0080".to_string(),
            kind: EvidenceKind::ToolCall,
            summary: "bash({\"command\": \"x\"})".to_string(),
            content_hash: None,
            recovery_handle: None,
            stable_id: None,
            checkpoint_id: None,
            source_ordinal: Some(80),
            tool_call_id: Some("call-1".to_string()),
        });
        let selected = select_materializable_evidence(&evidence, "ckpt-1");
        assert_eq!(selected.len(), MAX_EVIDENCE_REFS_PER_CHECKPOINT);
        assert!(selected
            .iter()
            .all(|item| !matches!(item.kind, EvidenceKind::ToolCall)));
        assert!(selected.iter().all(|item| item
            .stable_id
            .as_deref()
            .is_some_and(|id| id.contains("ckpt-1"))));
        assert!(selected
            .iter()
            .all(|item| item.checkpoint_id.as_deref() == Some("ckpt-1")));
    }

    #[test]
    fn selection_prioritizes_user_over_assistant() {
        let evidence = vec![
            EvidenceRef {
                id: "msg_0000".to_string(),
                kind: EvidenceKind::AssistantMessage,
                summary: "assistant chatter".to_string(),
                content_hash: Some("a".to_string()),
                recovery_handle: None,
                stable_id: None,
                checkpoint_id: None,
                source_ordinal: Some(0),
                tool_call_id: None,
            },
            EvidenceRef {
                id: "msg_0001".to_string(),
                kind: EvidenceKind::UserMessage,
                summary: "user steering".to_string(),
                content_hash: Some("b".to_string()),
                recovery_handle: None,
                stable_id: None,
                checkpoint_id: None,
                source_ordinal: Some(1),
                tool_call_id: None,
            },
        ];
        let selected = select_materializable_evidence(&evidence, "ckpt-9");
        assert_eq!(selected.len(), 2);
        // User steering sorts before assistant chatter in selection order,
        // but output is source-ordered; verify priority helper directly.
        assert!(
            evidence_priority(&EvidenceKind::UserMessage)
                < evidence_priority(&EvidenceKind::AssistantMessage)
        );
        assert!(selected
            .iter()
            .any(|item| matches!(item.kind, EvidenceKind::UserMessage)));
    }

    #[test]
    fn redaction_strips_secret_assignments_and_tokens() {
        let redacted = redact_evidence_text("api_key = \"supersecretvalue123\" then done");
        assert!(!redacted.contains("supersecretvalue123"));
        assert!(redacted.contains("[redacted]"));
        let redacted = redact_evidence_text("token sk-abcdefghijklmnop rest");
        assert!(!redacted.contains("sk-abcdefghijklmnop"));
        let redacted = redact_evidence_text("From https://user:s3cret@github.com/r.git");
        assert!(!redacted.contains("s3cret"));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn materialize_user_steering_creates_recoverable_ref() {
        let store = InMemoryArtifactStore::new();
        let messages = vec![user("origin task"), user("must keep parser strict")];
        let index = crate::context::compaction::build_evidence_index(&messages);
        let mut selected = select_materializable_evidence(&index, "ckpt-1");
        assert!(!selected.is_empty());
        let outcome =
            persist_selected_evidence(&store, "sess-1", "ckpt-1", 0, &messages, &mut selected, &[])
                .await;
        assert!(outcome.materialized >= 1);
        assert_eq!(outcome.reused, 0);
        let with_handle = selected
            .iter()
            .find(|item| item.recovery_handle.is_some())
            .expect("handle");
        assert!(with_handle
            .recovery_handle
            .as_deref()
            .unwrap()
            .starts_with("ctx://evidence/sess-1/ckpt-1/"));
        assert!(with_handle.stable_id.as_deref().unwrap().contains("ckpt-1"));
        // Digest matches the redacted stored body.
        let handle = with_handle.recovery_handle.clone().unwrap();
        let artifact = store.get(&handle).await.unwrap().unwrap();
        assert_eq!(artifact.kind, ArtifactKind::ContinuationEvidence);
        assert_eq!(
            artifact.content_hash,
            crate::context::compute_content_hash(&artifact.redacted_content)
        );
        assert_eq!(
            with_handle.content_hash.as_deref(),
            Some(artifact.content_hash.as_str())
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn existing_tool_handle_is_reused_not_duplicated() {
        let store = InMemoryArtifactStore::new();
        let tool_handle = ContextHandle::build_tool("sess-1", 0, "call-1").unwrap();
        store
            .put(ContextArtifact {
                handle: tool_handle.clone(),
                session_id: "sess-1".to_string(),
                turn_index: 0,
                tool_call_id: Some("call-1".to_string()),
                tool_name: Some("bash".to_string()),
                kind: ArtifactKind::ToolResult,
                created_at_ms: 1,
                content_hash: crate::context::compute_content_hash("test output"),
                redacted_content: "test output".to_string(),
                raw_bytes_len: 11,
                estimated_tokens: 3,
            })
            .await
            .unwrap();
        let messages = vec![user("origin"), tool_result("call-1", "test output")];
        let index = crate::context::compaction::build_evidence_index(&messages);
        let mut selected = select_materializable_evidence(&index, "ckpt-2");
        let outcome = persist_selected_evidence(
            &store,
            "sess-1",
            "ckpt-2",
            0,
            &messages,
            &mut selected,
            std::slice::from_ref(&tool_handle),
        )
        .await;
        assert!(outcome.reused >= 1);
        let tool_ref = selected
            .iter()
            .find(|item| item.tool_call_id.as_deref() == Some("call-1"))
            .expect("tool ref");
        assert_eq!(
            tool_ref.recovery_handle.as_deref(),
            Some(tool_handle.as_str())
        );
        // Stable identity remains checkpoint-scoped even when reusing.
        assert!(tool_ref.stable_id.as_deref().unwrap().contains("ckpt-2"));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn tool_call_arguments_are_never_persisted() {
        let store = InMemoryArtifactStore::new();
        let messages = vec![user("origin")];
        let evidence = vec![EvidenceRef {
            id: "tool_0000".to_string(),
            kind: EvidenceKind::ToolCall,
            summary: "bash({\"command\": \"x\", \"api_key\": \"sekret\"})".to_string(),
            content_hash: None,
            recovery_handle: None,
            stable_id: None,
            checkpoint_id: None,
            source_ordinal: Some(0),
            tool_call_id: Some("call-1".to_string()),
        }];
        let mut selected = evidence;
        let outcome =
            persist_selected_evidence(&store, "sess-1", "ckpt-3", 0, &messages, &mut selected, &[])
                .await;
        assert_eq!(outcome.materialized, 0);
        assert!(selected[0].recovery_handle.is_none());
        let listed = store.list_recent("sess-1", 10).await.unwrap();
        assert!(listed.is_empty());
    }

    #[tokio::test(flavor = "current_thread")]
    async fn reasoning_parts_are_excluded_from_evidence_body() {
        use std::sync::Arc;
        let store = InMemoryArtifactStore::new();
        let messages = vec![Message::Assistant {
            content: vec![
                ContentPart::Text {
                    text: Arc::new("visible decision".to_string()),
                },
                ContentPart::Reasoning {
                    text: Arc::new("hidden chain of thought".to_string()),
                    visibility: crate::provider::ReasoningVisibility::Private,
                },
            ],
            tool_calls: vec![],
        }];
        let index = crate::context::compaction::build_evidence_index(&messages);
        assert_eq!(index.len(), 1);
        assert!(index[0].summary.contains("visible decision"));
        assert!(!index[0].summary.contains("hidden chain"));
        let mut selected = select_materializable_evidence(&index, "ckpt-4");
        persist_selected_evidence(&store, "sess-1", "ckpt-4", 0, &messages, &mut selected, &[])
            .await;
        let handle = selected[0].recovery_handle.clone().unwrap();
        let artifact = store.get(&handle).await.unwrap().unwrap();
        assert!(artifact.redacted_content.contains("visible decision"));
        assert!(!artifact.redacted_content.contains("hidden chain"));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn verify_degrades_missing_to_summary_only() {
        let store = InMemoryArtifactStore::new();
        let mut refs = vec![EvidenceRef {
            id: "msg_0000".to_string(),
            kind: EvidenceKind::UserMessage,
            summary: "steering".to_string(),
            content_hash: Some("abc".to_string()),
            recovery_handle: Some("ctx://evidence/sess-1/ckpt-1/ev-user-0000-abc".to_string()),
            stable_id: Some("ckpt-1:ev-user-0000-abc".to_string()),
            checkpoint_id: Some("ckpt-1".to_string()),
            source_ordinal: Some(0),
            tool_call_id: None,
        }];
        let outcome = verify_evidence_artifacts(&store, "sess-1", &mut refs).await;
        assert_eq!(outcome.verified, 0);
        assert_eq!(outcome.degraded, 1);
        assert!(refs[0].recovery_handle.is_none());
        assert_eq!(refs[0].summary, "steering");
    }

    #[tokio::test(flavor = "current_thread")]
    async fn single_artifact_cap_enforced() {
        let store = InMemoryArtifactStore::new();
        let big = "y".repeat(MAX_SINGLE_EVIDENCE_BYTES + 1024);
        let messages = vec![user("origin"), assistant(&big)];
        let index = crate::context::compaction::build_evidence_index(&messages);
        let mut selected = select_materializable_evidence(&index, "ckpt-5");
        persist_selected_evidence(&store, "sess-1", "ckpt-5", 0, &messages, &mut selected, &[])
            .await;
        for item in &selected {
            if let Some(handle) = item.recovery_handle.as_deref() {
                let artifact = store.get(handle).await.unwrap().unwrap();
                assert!(artifact.redacted_content.len() <= MAX_SINGLE_EVIDENCE_BYTES);
            }
        }
    }
}

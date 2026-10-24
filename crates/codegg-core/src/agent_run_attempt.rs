//! A delegated run's attempt record and its continuation authorization.
//!
//! ADR-0051 §4 fixes the continuation authority: the spawning principal only,
//! under the parent's own authority and ceiling, one continuation per terminal
//! failure, a new attempt of the same run, and no provider entry until the
//! recorded transcript has been read back and its redaction re-verified.
//!
//! This module owns the **continuability decision**
//! ([`DelegatedFailureClass`]), the **attempt record** (an append-only history
//! of a run's attempts), and the **continuation authorization** (a typed,
//! claimable-once record of who may continue which run, under which parent's
//! authority). The last two are separate tables because they have different
//! shapes: an attempt row is never updated or deleted, while a continuation's
//! state moves from claimed to dispatched — or to needing attention, never a
//! replay.

use std::collections::HashMap;

use async_trait::async_trait;
use chrono::Utc;
use serde::{Deserialize, Serialize};
use sqlx::{Row, SqlitePool};
use thiserror::Error;

use super::agent_run_transcript::{
    redact_transcript_body, AgentRunTranscriptEntry, AgentRunTranscriptPage, TranscriptKind,
    MAX_TRANSCRIPT_ROW_BYTES,
};
use crate::identity::AgentRunId;

/// How long after a terminal failure the spawning principal may still claim
/// the continuation. The bound makes the authorization observable: a failure
/// the parent ignored for a day is not the failure it is looking at now.
pub const CONTINUATION_AUTHORIZATION_TTL_MS: i64 = 24 * 60 * 60 * 1000;

/// How long a claimed-but-undispatched continuation is left alone before the
/// reconciliation surfaces it as needing attention. The grace period keeps the
/// reconciliation from racing a dispatcher still in flight.
pub const CONTINUATION_CLAIM_GRACE_MS: i64 = 60_000;

/// The terminal failure classes a delegated run can end with, and this
/// milestone's recorded decision about which of them are continuable. The
/// decision is explicit per class, not inherited from a generic bucket. A class
/// the repository cannot produce maps to [`Self::Unclassified`], which is
/// **not** continuable: the classification fails closed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DelegatedFailureClass {
    /// The provider rejected the request's credentials.
    ///
    /// **Not continuable.** The continuation carries the parent's authority
    /// unchanged, so it presents the same credentials.
    ProviderAuthentication,
    /// The provider throttled the request.
    ///
    /// **Continuable.** Throttling is transient by the provider taxonomy the
    /// repository already owns.
    ProviderRateLimited,
    /// A transient upstream failure: a 5xx, a transport error, an interrupted
    /// stream.
    ///
    /// **Continuable.** The same taxonomy the provider-turn retry loop
    /// already treats as transient.
    ProviderTransient,
    /// The run's own deadline elapsed.
    ///
    /// **Continuable.** A fresh attempt gets a fresh deadline; the work
    /// itself is unchanged.
    DeadlineExceeded,
    /// The run's own budget was exhausted.
    ///
    /// **Continuable.** A fresh attempt gets a fresh budget.
    BudgetExhausted,
    /// A tool the child needed was refused by the permission surface.
    ///
    /// **Continuable.** The parent's own tool policy is carried through
    /// unchanged, so the continuation is the parent's chance to re-dispatch.
    ToolPermissionDenied,
    /// The run was cancelled.
    ///
    /// **Never continuable.** A cancellation is a decision, not a failure.
    Cancelled,
    /// A failure with no recognised class.
    ///
    /// **Not continuable.** An unrecognised failure is not assumed
    /// recoverable.
    Unclassified,
}

impl DelegatedFailureClass {
    /// The canonical durable spelling of a class.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::ProviderAuthentication => "provider_authentication",
            Self::ProviderRateLimited => "provider_rate_limit",
            Self::ProviderTransient => "provider_transient",
            Self::DeadlineExceeded => "deadline_exceeded",
            Self::BudgetExhausted => "budget_exhausted",
            Self::ToolPermissionDenied => "tool_permission_denied",
            Self::Cancelled => "cancelled",
            Self::Unclassified => "unclassified",
        }
    }

    /// The class a durable failure record names.
    ///
    /// An unknown spelling is [`Self::Unclassified`]: the classification fails
    /// closed rather than guessing.
    pub fn parse(value: &str) -> Self {
        match value {
            "provider_authentication" => Self::ProviderAuthentication,
            "provider_rate_limit" => Self::ProviderRateLimited,
            "provider_transient" => Self::ProviderTransient,
            "deadline_exceeded" => Self::DeadlineExceeded,
            "budget_exhausted" => Self::BudgetExhausted,
            "tool_permission_denied" => Self::ToolPermissionDenied,
            "cancelled" => Self::Cancelled,
            _ => Self::Unclassified,
        }
    }

    /// The recorded decision: may a run that failed this way be continued?
    pub fn is_continuable(self) -> bool {
        matches!(
            self,
            Self::ProviderRateLimited
                | Self::ProviderTransient
                | Self::DeadlineExceeded
                | Self::BudgetExhausted
                | Self::ToolPermissionDenied
        )
    }

    /// The class a durable failure record names, or [`Self::Unclassified`].
    pub fn classify(failure_class: Option<&str>) -> Self {
        failure_class.map_or(Self::Unclassified, Self::parse)
    }
}

/// Why a continuation request was refused.
///
/// Every variant is a typed reason the orchestrator can read. A refusal is
/// always decided **before** any provider entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ContinuationRefusal {
    /// The run id does not resolve to a delegated run.
    RunNotFound,
    /// The run has not reached a terminal state, so there is no failure to
    /// continue.
    NotTerminal,
    /// The run's parent is gone, so nobody holds its authority any more.
    NoLiveParent,
    /// The actor is not the principal that spawned the run.
    WrongPrincipal,
    /// The run was cancelled. A cancellation is a decision, not a failure.
    RunCancelled,
    /// The run's terminal outcome was superseded by a different outcome, so
    /// the failure the caller wants to continue is stale.
    RunSuperseded,
    /// The continuation authorization for this failure has expired.
    AuthorizationExpired,
    /// A continuation for this run was already claimed. A duplicate request
    /// is refused, never replayed.
    AlreadyContinued,
    /// The terminal failure is not one this milestone's recorded decision
    /// designates as continuable.
    NotContinuable(DelegatedFailureClass),
    /// The run has no recorded transcript, so a continuation would start from
    /// nothing and present itself as a continuation.
    TranscriptAbsent,
    /// A stored transcript row failed re-verification, so it is refused rather
    /// than seeded.
    TranscriptUnverified,
    /// The request asked for a ceiling wider than the attempt's own.
    CeilingWidened,
    /// The attempt recorded no ceiling, so the parent's authority cannot be
    /// carried through unchanged.
    CeilingUnavailable,
    /// The durable store failed.
    Storage(String),
}

impl ContinuationRefusal {
    /// The stable, machine-readable code for this refusal.
    pub fn code(&self) -> &'static str {
        match self {
            Self::RunNotFound => "run_not_found",
            Self::NotTerminal => "not_terminal",
            Self::NoLiveParent => "no_live_parent",
            Self::WrongPrincipal => "wrong_principal",
            Self::RunCancelled => "run_cancelled",
            Self::RunSuperseded => "run_superseded",
            Self::AuthorizationExpired => "authorization_expired",
            Self::AlreadyContinued => "already_continued",
            Self::NotContinuable(_) => "not_continuable",
            Self::TranscriptAbsent => "transcript_absent",
            Self::TranscriptUnverified => "transcript_unverified",
            Self::CeilingWidened => "ceiling_widened",
            Self::CeilingUnavailable => "ceiling_unavailable",
            Self::Storage(_) => "storage",
        }
    }
}

impl std::fmt::Display for ContinuationRefusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::RunNotFound => write!(f, "the delegated run does not exist"),
            Self::NotTerminal => {
                write!(f, "the delegated run has not reached a terminal state")
            }
            Self::NoLiveParent => write!(f, "the delegated run's parent is gone"),
            Self::WrongPrincipal => {
                write!(f, "only the principal that spawned the run may continue it")
            }
            Self::RunCancelled => write!(f, "a cancelled run is never continued"),
            Self::RunSuperseded => write!(
                f,
                "the delegated run's terminal outcome was superseded, so the failure is stale"
            ),
            Self::AuthorizationExpired => {
                write!(
                    f,
                    "the continuation authorization for this failure has expired"
                )
            }
            Self::AlreadyContinued => write!(
                f,
                "the delegated run was already continued; a duplicate request is refused"
            ),
            Self::NotContinuable(class) => write!(
                f,
                "the terminal failure ({}) is not continuable under the recorded decision",
                class.as_str()
            ),
            Self::TranscriptAbsent => write!(
                f,
                "the delegated run has no recorded transcript, so a continuation would start \
                 from nothing"
            ),
            Self::TranscriptUnverified => write!(
                f,
                "a stored transcript row failed redaction re-verification and is refused"
            ),
            Self::CeilingWidened => {
                write!(
                    f,
                    "the request asks for a ceiling wider than the attempt's own"
                )
            }
            Self::CeilingUnavailable => write!(
                f,
                "the attempt recorded no ceiling, so the parent's authority cannot be carried \
                 through unchanged"
            ),
            Self::Storage(detail) => write!(f, "the durable store failed: {detail}"),
        }
    }
}

/// The kind of an attempt: the run's first, or a continuation of it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AgentRunAttemptKind {
    /// The run's first attempt.
    Original,
    /// A continuation of a terminal failure: a new attempt of the same run.
    Continuation,
}

impl AgentRunAttemptKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Original => "original",
            Self::Continuation => "continuation",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "original" => Some(Self::Original),
            "continuation" => Some(Self::Continuation),
            _ => None,
        }
    }
}

/// What an attempt record row records.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AgentRunAttemptEvent {
    /// The attempt was created. This row is the compare-and-swap: its unique
    /// key decides which request wins the continuation.
    Attempt,
    /// The attempt reached a terminal outcome.
    Outcome,
    /// The reconciliation surfaced the attempt as needing attention. It is
    /// never replayed.
    Attention,
}

impl AgentRunAttemptEvent {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Attempt => "attempt",
            Self::Outcome => "outcome",
            Self::Attention => "attention",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "attempt" => Some(Self::Attempt),
            "outcome" => Some(Self::Outcome),
            "attention" => Some(Self::Attention),
            _ => None,
        }
    }
}

/// One row of a run's append-only attempt record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentRunAttemptRecord {
    pub run_id: AgentRunId,
    pub attempt_ordinal: i64,
    pub event: AgentRunAttemptEvent,
    pub attempt_kind: AgentRunAttemptKind,
    pub continued_from_ordinal: Option<i64>,
    /// The principal that continued the run. Empty for the original attempt.
    pub principal: String,
    pub principal_turn_id: String,
    /// The parent's ceiling digest, carried through unchanged.
    pub authority_digest: String,
    /// The digest of the transcript that was seeded into the attempt.
    pub transcript_digest: Option<String>,
    pub failure_class: Option<DelegatedFailureClass>,
    /// The attempt's ceiling, verbatim: the payload the attempt ran under.
    pub ceiling_json: Option<String>,
    /// The attempt's outcome, on an [`AgentRunAttemptEvent::Outcome`] row.
    pub outcome: Option<String>,
    pub detail: Option<String>,
    pub created_at: i64,
}

/// A continuation's state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ContinuationState {
    /// The continuation was claimed and has not been dispatched yet.
    Claimed,
    /// The continuation was dispatched: a new attempt of the same run.
    Dispatched,
    /// The continuation was claimed but its dispatch never happened, or its
    /// dispatch cannot be proven. It is surfaced, never replayed.
    NeedsAttention,
}

impl ContinuationState {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Claimed => "claimed",
            Self::Dispatched => "dispatched",
            Self::NeedsAttention => "needs_attention",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "claimed" => Some(Self::Claimed),
            "dispatched" => Some(Self::Dispatched),
            "needs_attention" => Some(Self::NeedsAttention),
            _ => None,
        }
    }
}

/// The continuation authorization: a typed record of who may continue which
/// run, under which parent's authority.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentRunContinuationRecord {
    pub run_id: AgentRunId,
    pub attempt_ordinal: i64,
    pub state: ContinuationState,
    pub principal: String,
    pub principal_turn_id: String,
    pub authority_digest: String,
    pub transcript_digest: Option<String>,
    pub failure_class: DelegatedFailureClass,
    pub detail: Option<String>,
    pub created_at: i64,
    pub updated_at: i64,
}

/// The claim a continuation request makes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewContinuationClaim {
    pub run_id: AgentRunId,
    /// The ordinal the continuation creates.
    pub attempt_ordinal: i64,
    pub attempt_kind: AgentRunAttemptKind,
    pub continued_from_ordinal: Option<i64>,
    pub principal: String,
    pub principal_turn_id: String,
    pub authority_digest: String,
    pub transcript_digest: Option<String>,
    pub failure_class: DelegatedFailureClass,
}

/// The result of a claim: exactly one request wins it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ContinuationClaimOutcome {
    /// This request won the claim.
    Claimed(AgentRunContinuationRecord),
    /// Another request won it first. The duplicate is refused, never replayed.
    AlreadyClaimed(AgentRunContinuationRecord),
}

/// The durable store of a delegated run's attempts and continuations.
#[async_trait]
pub trait AgentRunAttemptStore: Send + Sync {
    /// Claim a continuation. The compare-and-swap is the unique key of the
    /// attempt row: exactly one request wins, and every other request is told
    /// it lost.
    async fn claim_continuation(
        &self,
        claim: NewContinuationClaim,
    ) -> Result<ContinuationClaimOutcome, AgentRunAttemptError>;

    /// Append an attempt record row. Append-only: a row is never updated and
    /// never deleted.
    async fn append_attempt(
        &self,
        record: NewAgentRunAttempt,
    ) -> Result<AgentRunAttemptRecord, AgentRunAttemptError>;

    /// Every attempt row for a run, oldest first.
    async fn attempts(
        &self,
        run_id: &AgentRunId,
    ) -> Result<Vec<AgentRunAttemptRecord>, AgentRunAttemptError>;

    /// The run's current attempt ordinal. A run with no attempt record reads
    /// as `1`: it is the run's first attempt.
    async fn attempt_ordinal(&self, run_id: &AgentRunId) -> Result<i64, AgentRunAttemptError>;

    /// The continuation authorization for a run, if one exists.
    async fn continuation(
        &self,
        run_id: &AgentRunId,
    ) -> Result<Option<AgentRunContinuationRecord>, AgentRunAttemptError>;

    /// Every continuation authorization, for the reconciliation.
    async fn continuations(&self) -> Result<Vec<AgentRunContinuationRecord>, AgentRunAttemptError>;

    /// Move a claimed continuation to `dispatched`.
    async fn mark_dispatched(
        &self,
        run_id: &AgentRunId,
        attempt_ordinal: i64,
    ) -> Result<bool, AgentRunAttemptError>;

    /// Move a claimed continuation to `needs_attention`. It is never replayed.
    async fn mark_needs_attention(
        &self,
        run_id: &AgentRunId,
        attempt_ordinal: i64,
        detail: &str,
    ) -> Result<bool, AgentRunAttemptError>;

    /// Release a claim that was refused before any dispatch, so the one
    /// continuation a failure is allowed is not consumed by a refusal.
    async fn release_claim(
        &self,
        run_id: &AgentRunId,
        attempt_ordinal: i64,
    ) -> Result<bool, AgentRunAttemptError>;
}

/// A new attempt record row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewAgentRunAttempt {
    pub run_id: AgentRunId,
    pub attempt_ordinal: i64,
    pub event: AgentRunAttemptEvent,
    pub attempt_kind: AgentRunAttemptKind,
    pub continued_from_ordinal: Option<i64>,
    pub principal: String,
    pub principal_turn_id: String,
    pub authority_digest: String,
    pub transcript_digest: Option<String>,
    pub failure_class: Option<DelegatedFailureClass>,
    pub ceiling_json: Option<String>,
    pub outcome: Option<String>,
    pub detail: Option<String>,
}

/// The durable store's failures.
#[derive(Debug, Error)]
pub enum AgentRunAttemptError {
    #[error("storage failure: {0}")]
    Storage(String),
    /// The claim lost: another request claimed the continuation first.
    #[error("the continuation was already claimed")]
    AlreadyClaimed,
    /// The attempt row the request needed does not exist.
    #[error("attempt {0} of run {1} does not exist")]
    AttemptNotFound(i64, String),
}

/// The digest of a transcript page: what a continuation records as the
/// transcript it seeded from.
pub fn transcript_digest(entries: &[AgentRunTranscriptEntry]) -> String {
    let mut digest = String::new();
    for entry in entries {
        digest.push_str(&entry.ordinal.to_string());
        digest.push(':');
        digest.push_str(entry.redacted_body.as_str());
        digest.push('\n');
    }
    // A stable, bounded digest. The repository's own hasher is not needed
    // here: the digest identifies the page, it is not a security boundary.
    let bytes = digest.as_bytes();
    let mut hash: u64 = 0xcbf29ce484222325;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    format!("{hash:016x}:{}", entries.len())
}

/// Why a stored transcript row is refused rather than seeded.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TranscriptVerificationError {
    /// The stored body is not fully redacted: re-running the repository's own
    /// recogniser over it changes it.
    NotRedacted,
    /// The stored body exceeds the per-row bound.
    TooLong,
}

/// Re-verify that a stored transcript row is already fully redacted.
///
/// The recogniser is the repository's existing one — the same
/// [`redact_transcript_body`] the store applies at write time. A row that
/// re-redacts to something else was not redacted when it was written, so it
/// is refused rather than seeded.
pub fn reverify_transcript_row(
    kind: TranscriptKind,
    stored_body: &str,
) -> Result<(), TranscriptVerificationError> {
    if stored_body.len() > MAX_TRANSCRIPT_ROW_BYTES {
        return Err(TranscriptVerificationError::TooLong);
    }
    if redact_transcript_body(kind, stored_body) != stored_body {
        return Err(TranscriptVerificationError::NotRedacted);
    }
    Ok(())
}

/// The seeded context of a continuation: exactly the stored transcript.
///
/// The context is the stored rows in ordinal order, each labelled with its
/// role and kind so the child can tell the orchestrator-visible channel from
/// its own. Nothing that was not stored is added, and a truncated transcript
/// says so rather than pretending the tail is complete.
pub fn build_seeded_context(page: &AgentRunTranscriptPage) -> String {
    let mut context = String::new();
    for entry in &page.entries {
        if !context.is_empty() {
            context.push('\n');
        }
        context.push_str(entry.role.as_str());
        context.push('/');
        context.push_str(entry.kind.as_str());
        context.push_str(": ");
        context.push_str(&entry.redacted_body);
    }
    if page.truncated {
        if !context.is_empty() {
            context.push('\n');
        }
        context.push_str(
            "[the recorded transcript is truncated; the missing tail was never recorded]",
        );
    }
    context
}

/// The in-memory store, for tests and for callers that hold no database.
#[derive(Default)]
pub struct InMemoryAgentRunAttemptStore {
    state: tokio::sync::Mutex<MemoryAttemptState>,
}

#[derive(Default)]
struct MemoryAttemptState {
    attempts: Vec<AgentRunAttemptRecord>,
    continuations: HashMap<AgentRunId, AgentRunContinuationRecord>,
}

impl InMemoryAgentRunAttemptStore {
    pub fn new() -> Self {
        Self::default()
    }
}

fn now_ms() -> i64 {
    Utc::now().timestamp_millis()
}

fn attempt_record_from_new(new: &NewAgentRunAttempt) -> AgentRunAttemptRecord {
    AgentRunAttemptRecord {
        run_id: new.run_id.clone(),
        attempt_ordinal: new.attempt_ordinal,
        event: new.event,
        attempt_kind: new.attempt_kind,
        continued_from_ordinal: new.continued_from_ordinal,
        principal: new.principal.clone(),
        principal_turn_id: new.principal_turn_id.clone(),
        authority_digest: new.authority_digest.clone(),
        transcript_digest: new.transcript_digest.clone(),
        failure_class: new.failure_class,
        ceiling_json: new.ceiling_json.clone(),
        outcome: new.outcome.clone(),
        detail: new.detail.clone(),
        created_at: now_ms(),
    }
}

#[async_trait]
impl AgentRunAttemptStore for InMemoryAgentRunAttemptStore {
    async fn claim_continuation(
        &self,
        claim: NewContinuationClaim,
    ) -> Result<ContinuationClaimOutcome, AgentRunAttemptError> {
        let mut state = self.state.lock().await;
        if let Some(existing) = state.continuations.get(&claim.run_id) {
            return Ok(ContinuationClaimOutcome::AlreadyClaimed(existing.clone()));
        }
        let now = now_ms();
        let record = AgentRunContinuationRecord {
            run_id: claim.run_id.clone(),
            attempt_ordinal: claim.attempt_ordinal,
            state: ContinuationState::Claimed,
            principal: claim.principal.clone(),
            principal_turn_id: claim.principal_turn_id.clone(),
            authority_digest: claim.authority_digest.clone(),
            transcript_digest: claim.transcript_digest.clone(),
            failure_class: claim.failure_class,
            detail: None,
            created_at: now,
            updated_at: now,
        };
        state
            .continuations
            .insert(claim.run_id.clone(), record.clone());
        Ok(ContinuationClaimOutcome::Claimed(record))
    }

    async fn append_attempt(
        &self,
        record: NewAgentRunAttempt,
    ) -> Result<AgentRunAttemptRecord, AgentRunAttemptError> {
        let mut state = self.state.lock().await;
        if state.attempts.iter().any(|row| {
            row.run_id == record.run_id
                && row.attempt_ordinal == record.attempt_ordinal
                && row.event == record.event
        }) {
            return Err(AgentRunAttemptError::AlreadyClaimed);
        }
        let stored = attempt_record_from_new(&record);
        state.attempts.push(stored.clone());
        Ok(stored)
    }

    async fn attempts(
        &self,
        run_id: &AgentRunId,
    ) -> Result<Vec<AgentRunAttemptRecord>, AgentRunAttemptError> {
        let state = self.state.lock().await;
        let mut rows: Vec<AgentRunAttemptRecord> = state
            .attempts
            .iter()
            .filter(|row| row.run_id == *run_id)
            .cloned()
            .collect();
        rows.sort_by_key(|row| (row.attempt_ordinal, row.event.as_str()));
        Ok(rows)
    }

    async fn attempt_ordinal(&self, run_id: &AgentRunId) -> Result<i64, AgentRunAttemptError> {
        let state = self.state.lock().await;
        let ordinal = state
            .attempts
            .iter()
            .filter(|row| row.run_id == *run_id && row.event == AgentRunAttemptEvent::Attempt)
            .map(|row| row.attempt_ordinal)
            .max()
            .unwrap_or(1);
        Ok(ordinal)
    }

    async fn continuation(
        &self,
        run_id: &AgentRunId,
    ) -> Result<Option<AgentRunContinuationRecord>, AgentRunAttemptError> {
        let state = self.state.lock().await;
        Ok(state.continuations.get(run_id).cloned())
    }

    async fn continuations(&self) -> Result<Vec<AgentRunContinuationRecord>, AgentRunAttemptError> {
        let state = self.state.lock().await;
        let mut records: Vec<AgentRunContinuationRecord> =
            state.continuations.values().cloned().collect();
        records.sort_by_key(|record| record.created_at);
        Ok(records)
    }

    async fn mark_dispatched(
        &self,
        run_id: &AgentRunId,
        attempt_ordinal: i64,
    ) -> Result<bool, AgentRunAttemptError> {
        let mut state = self.state.lock().await;
        let Some(record) = state.continuations.get_mut(run_id) else {
            return Ok(false);
        };
        if record.attempt_ordinal != attempt_ordinal {
            return Ok(false);
        }
        record.state = ContinuationState::Dispatched;
        record.updated_at = now_ms();
        Ok(true)
    }

    async fn mark_needs_attention(
        &self,
        run_id: &AgentRunId,
        attempt_ordinal: i64,
        detail: &str,
    ) -> Result<bool, AgentRunAttemptError> {
        let mut state = self.state.lock().await;
        let Some(record) = state.continuations.get_mut(run_id) else {
            return Ok(false);
        };
        if record.attempt_ordinal != attempt_ordinal {
            return Ok(false);
        }
        record.state = ContinuationState::NeedsAttention;
        record.detail = Some(detail.to_owned());
        record.updated_at = now_ms();
        Ok(true)
    }

    async fn release_claim(
        &self,
        run_id: &AgentRunId,
        attempt_ordinal: i64,
    ) -> Result<bool, AgentRunAttemptError> {
        let mut state = self.state.lock().await;
        let Some(record) = state.continuations.get_mut(run_id) else {
            return Ok(false);
        };
        if record.attempt_ordinal != attempt_ordinal || record.state != ContinuationState::Claimed {
            return Ok(false);
        }
        state.continuations.remove(run_id);
        Ok(true)
    }
}

/// The durable store, over the session database.
pub struct SqliteAgentRunAttemptStore {
    pool: SqlitePool,
}

impl SqliteAgentRunAttemptStore {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }
}

fn storage_error(error: impl std::fmt::Display) -> AgentRunAttemptError {
    AgentRunAttemptError::Storage(error.to_string())
}

fn is_unique_violation(error: &sqlx::Error) -> bool {
    match error {
        sqlx::Error::Database(db) => db.is_unique_violation(),
        _ => false,
    }
}

const ATTEMPT_COLUMNS: &str = "run_id, attempt_ordinal, event, attempt_kind, \
     continued_from_ordinal, principal, principal_turn_id, authority_digest, \
     transcript_digest, failure_class, ceiling_json, outcome, detail, created_at";

fn row_to_attempt(
    row: &sqlx::sqlite::SqliteRow,
) -> Result<AgentRunAttemptRecord, AgentRunAttemptError> {
    let event = AgentRunAttemptEvent::parse(&row.get::<String, _>("event"));
    Ok(AgentRunAttemptRecord {
        run_id: AgentRunId::parse(&row.get::<String, _>("run_id")).map_err(storage_error)?,
        attempt_ordinal: row.get::<i64, _>("attempt_ordinal"),
        event: event.unwrap_or(AgentRunAttemptEvent::Attempt),
        attempt_kind: AgentRunAttemptKind::parse(&row.get::<String, _>("attempt_kind"))
            .unwrap_or(AgentRunAttemptKind::Original),
        continued_from_ordinal: row.get::<Option<i64>, _>("continued_from_ordinal"),
        principal: row.get::<String, _>("principal"),
        principal_turn_id: row.get::<String, _>("principal_turn_id"),
        authority_digest: row.get::<String, _>("authority_digest"),
        transcript_digest: row.get::<Option<String>, _>("transcript_digest"),
        failure_class: row
            .get::<Option<String>, _>("failure_class")
            .map(|value| DelegatedFailureClass::parse(&value)),
        ceiling_json: row.get::<Option<String>, _>("ceiling_json"),
        outcome: row.get::<Option<String>, _>("outcome"),
        detail: row.get::<Option<String>, _>("detail"),
        created_at: row.get::<i64, _>("created_at"),
    })
}

#[async_trait]
impl AgentRunAttemptStore for SqliteAgentRunAttemptStore {
    async fn claim_continuation(
        &self,
        claim: NewContinuationClaim,
    ) -> Result<ContinuationClaimOutcome, AgentRunAttemptError> {
        let mut tx = self.pool.begin().await.map_err(storage_error)?;
        let now = now_ms();
        let inserted = sqlx::query(
            "INSERT INTO agent_run_continuation (run_id, attempt_ordinal, state, principal, \
             principal_turn_id, authority_digest, transcript_digest, failure_class, detail, \
             created_at, updated_at) VALUES (?, ?, 'claimed', ?, ?, ?, ?, ?, NULL, ?, ?)",
        )
        .bind(claim.run_id.as_str())
        .bind(claim.attempt_ordinal)
        .bind(&claim.principal)
        .bind(&claim.principal_turn_id)
        .bind(&claim.authority_digest)
        .bind(&claim.transcript_digest)
        .bind(claim.failure_class.as_str())
        .bind(now)
        .bind(now)
        .execute(&mut *tx)
        .await;
        let inserted = match inserted {
            Ok(result) => result,
            Err(error) if is_unique_violation(&error) => {
                // The claim lost. Read the winner's record and report it.
                tx.rollback().await.map_err(storage_error)?;
                let existing = self
                    .continuation(&claim.run_id)
                    .await?
                    .ok_or(AgentRunAttemptError::AlreadyClaimed)?;
                return Ok(ContinuationClaimOutcome::AlreadyClaimed(existing));
            }
            Err(error) => return Err(storage_error(error)),
        };
        let _ = inserted;
        tx.commit().await.map_err(storage_error)?;
        Ok(ContinuationClaimOutcome::Claimed(
            AgentRunContinuationRecord {
                run_id: claim.run_id.clone(),
                attempt_ordinal: claim.attempt_ordinal,
                state: ContinuationState::Claimed,
                principal: claim.principal.clone(),
                principal_turn_id: claim.principal_turn_id.clone(),
                authority_digest: claim.authority_digest.clone(),
                transcript_digest: claim.transcript_digest.clone(),
                failure_class: claim.failure_class,
                detail: None,
                created_at: now,
                updated_at: now,
            },
        ))
    }

    async fn append_attempt(
        &self,
        record: NewAgentRunAttempt,
    ) -> Result<AgentRunAttemptRecord, AgentRunAttemptError> {
        let now = now_ms();
        sqlx::query(
            "INSERT INTO agent_run_attempt (run_id, attempt_ordinal, event, attempt_kind, \
             continued_from_ordinal, principal, principal_turn_id, authority_digest, \
             transcript_digest, failure_class, ceiling_json, outcome, detail, created_at) \
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(record.run_id.as_str())
        .bind(record.attempt_ordinal)
        .bind(record.event.as_str())
        .bind(record.attempt_kind.as_str())
        .bind(record.continued_from_ordinal)
        .bind(&record.principal)
        .bind(&record.principal_turn_id)
        .bind(&record.authority_digest)
        .bind(&record.transcript_digest)
        .bind(record.failure_class.map(|class| class.as_str()))
        .bind(&record.ceiling_json)
        .bind(&record.outcome)
        .bind(&record.detail)
        .bind(now)
        .execute(&self.pool)
        .await
        .map_err(|error| {
            if is_unique_violation(&error) {
                AgentRunAttemptError::AlreadyClaimed
            } else {
                storage_error(error)
            }
        })?;
        Ok(AgentRunAttemptRecord {
            run_id: record.run_id.clone(),
            attempt_ordinal: record.attempt_ordinal,
            event: record.event,
            attempt_kind: record.attempt_kind,
            continued_from_ordinal: record.continued_from_ordinal,
            principal: record.principal.clone(),
            principal_turn_id: record.principal_turn_id.clone(),
            authority_digest: record.authority_digest.clone(),
            transcript_digest: record.transcript_digest.clone(),
            failure_class: record.failure_class,
            ceiling_json: record.ceiling_json.clone(),
            outcome: record.outcome.clone(),
            detail: record.detail.clone(),
            created_at: now,
        })
    }

    async fn attempts(
        &self,
        run_id: &AgentRunId,
    ) -> Result<Vec<AgentRunAttemptRecord>, AgentRunAttemptError> {
        let sql = format!(
            "SELECT {ATTEMPT_COLUMNS} FROM agent_run_attempt WHERE run_id = ? \
             ORDER BY attempt_ordinal ASC, event ASC"
        );
        let rows = sqlx::query(&sql)
            .bind(run_id.as_str())
            .fetch_all(&self.pool)
            .await
            .map_err(storage_error)?;
        rows.iter().map(row_to_attempt).collect()
    }

    async fn attempt_ordinal(&self, run_id: &AgentRunId) -> Result<i64, AgentRunAttemptError> {
        let ordinal: Option<i64> = sqlx::query_scalar(
            "SELECT MAX(attempt_ordinal) FROM agent_run_attempt \
             WHERE run_id = ? AND event = 'attempt'",
        )
        .bind(run_id.as_str())
        .fetch_one(&self.pool)
        .await
        .map_err(storage_error)?;
        Ok(ordinal.unwrap_or(1))
    }

    async fn continuation(
        &self,
        run_id: &AgentRunId,
    ) -> Result<Option<AgentRunContinuationRecord>, AgentRunAttemptError> {
        let row = sqlx::query(
            "SELECT run_id, attempt_ordinal, state, principal, principal_turn_id, \
             authority_digest, transcript_digest, failure_class, detail, created_at, \
             updated_at FROM agent_run_continuation WHERE run_id = ?",
        )
        .bind(run_id.as_str())
        .fetch_optional(&self.pool)
        .await
        .map_err(storage_error)?;
        let Some(row) = row else {
            return Ok(None);
        };
        Ok(Some(AgentRunContinuationRecord {
            run_id: AgentRunId::parse(&row.get::<String, _>("run_id")).map_err(storage_error)?,
            attempt_ordinal: row.get::<i64, _>("attempt_ordinal"),
            state: ContinuationState::parse(&row.get::<String, _>("state"))
                .unwrap_or(ContinuationState::Claimed),
            principal: row.get::<String, _>("principal"),
            principal_turn_id: row.get::<String, _>("principal_turn_id"),
            authority_digest: row.get::<String, _>("authority_digest"),
            transcript_digest: row.get::<Option<String>, _>("transcript_digest"),
            failure_class: DelegatedFailureClass::parse(&row.get::<String, _>("failure_class")),
            detail: row.get::<Option<String>, _>("detail"),
            created_at: row.get::<i64, _>("created_at"),
            updated_at: row.get::<i64, _>("updated_at"),
        }))
    }

    async fn continuations(&self) -> Result<Vec<AgentRunContinuationRecord>, AgentRunAttemptError> {
        let rows = sqlx::query(
            "SELECT run_id, attempt_ordinal, state, principal, principal_turn_id, \
             authority_digest, transcript_digest, failure_class, detail, created_at, \
             updated_at FROM agent_run_continuation ORDER BY created_at ASC",
        )
        .fetch_all(&self.pool)
        .await
        .map_err(storage_error)?;
        let mut records = Vec::with_capacity(rows.len());
        for row in rows {
            records.push(AgentRunContinuationRecord {
                run_id: AgentRunId::parse(&row.get::<String, _>("run_id"))
                    .map_err(storage_error)?,
                attempt_ordinal: row.get::<i64, _>("attempt_ordinal"),
                state: ContinuationState::parse(&row.get::<String, _>("state"))
                    .unwrap_or(ContinuationState::Claimed),
                principal: row.get::<String, _>("principal"),
                principal_turn_id: row.get::<String, _>("principal_turn_id"),
                authority_digest: row.get::<String, _>("authority_digest"),
                transcript_digest: row.get::<Option<String>, _>("transcript_digest"),
                failure_class: DelegatedFailureClass::parse(&row.get::<String, _>("failure_class")),
                detail: row.get::<Option<String>, _>("detail"),
                created_at: row.get::<i64, _>("created_at"),
                updated_at: row.get::<i64, _>("updated_at"),
            });
        }
        Ok(records)
    }

    async fn mark_dispatched(
        &self,
        run_id: &AgentRunId,
        attempt_ordinal: i64,
    ) -> Result<bool, AgentRunAttemptError> {
        let result = sqlx::query(
            "UPDATE agent_run_continuation SET state = 'dispatched', updated_at = ? \
             WHERE run_id = ? AND attempt_ordinal = ? AND state = 'claimed'",
        )
        .bind(now_ms())
        .bind(run_id.as_str())
        .bind(attempt_ordinal)
        .execute(&self.pool)
        .await
        .map_err(storage_error)?;
        Ok(result.rows_affected() == 1)
    }

    async fn mark_needs_attention(
        &self,
        run_id: &AgentRunId,
        attempt_ordinal: i64,
        detail: &str,
    ) -> Result<bool, AgentRunAttemptError> {
        let result = sqlx::query(
            "UPDATE agent_run_continuation SET state = 'needs_attention', detail = ?, \
             updated_at = ? WHERE run_id = ? AND attempt_ordinal = ? AND state = 'claimed'",
        )
        .bind(detail)
        .bind(now_ms())
        .bind(run_id.as_str())
        .bind(attempt_ordinal)
        .execute(&self.pool)
        .await
        .map_err(storage_error)?;
        Ok(result.rows_affected() == 1)
    }

    async fn release_claim(
        &self,
        run_id: &AgentRunId,
        attempt_ordinal: i64,
    ) -> Result<bool, AgentRunAttemptError> {
        let result = sqlx::query(
            "DELETE FROM agent_run_continuation WHERE run_id = ? AND attempt_ordinal = ? \
             AND state = 'claimed'",
        )
        .bind(run_id.as_str())
        .bind(attempt_ordinal)
        .execute(&self.pool)
        .await
        .map_err(storage_error)?;
        Ok(result.rows_affected() == 1)
    }
}

#[cfg(test)]
mod tests {
    use super::super::agent_run_transcript::TranscriptRole;
    use super::*;

    #[test]
    fn the_recorded_decision_names_each_class_explicitly() {
        assert!(
            !DelegatedFailureClass::ProviderAuthentication.is_continuable(),
            "an authentication failure cannot succeed on a retry with the same credentials"
        );
        assert!(DelegatedFailureClass::ProviderRateLimited.is_continuable());
        assert!(DelegatedFailureClass::ProviderTransient.is_continuable());
        assert!(DelegatedFailureClass::DeadlineExceeded.is_continuable());
        assert!(DelegatedFailureClass::BudgetExhausted.is_continuable());
        assert!(DelegatedFailureClass::ToolPermissionDenied.is_continuable());
        assert!(
            !DelegatedFailureClass::Cancelled.is_continuable(),
            "a cancellation is a decision, not a failure"
        );
        assert!(
            !DelegatedFailureClass::Unclassified.is_continuable(),
            "an unrecognised failure must not be assumed recoverable"
        );
    }

    #[test]
    fn an_unknown_failure_class_fails_closed() {
        assert_eq!(
            DelegatedFailureClass::classify(Some("something-new")),
            DelegatedFailureClass::Unclassified
        );
        assert_eq!(
            DelegatedFailureClass::classify(None),
            DelegatedFailureClass::Unclassified
        );
        assert_eq!(
            DelegatedFailureClass::parse("provider_rate_limit"),
            DelegatedFailureClass::ProviderRateLimited
        );
    }

    #[test]
    fn a_stored_row_that_is_not_redacted_is_refused() {
        let secret = "api_key=fixture-api-key-value";
        let redacted = redact_transcript_body(TranscriptKind::ToolResult, secret);
        assert_ne!(redacted, secret, "the recogniser must change a secret");
        assert_eq!(
            reverify_transcript_row(TranscriptKind::ToolResult, &redacted),
            Ok(()),
            "a properly redacted row must re-verify"
        );
        assert_eq!(
            reverify_transcript_row(TranscriptKind::ToolResult, secret),
            Err(TranscriptVerificationError::NotRedacted),
            "a row that still holds a secret must be refused"
        );
    }

    #[test]
    fn the_seeded_context_is_exactly_the_stored_rows() {
        let page = AgentRunTranscriptPage {
            entries: vec![
                AgentRunTranscriptEntry {
                    run_id: AgentRunId::new(),
                    ordinal: 0,
                    role: TranscriptRole::Orchestrator,
                    kind: TranscriptKind::ModelMessage,
                    redacted_body: "the prompt".to_string(),
                    byte_len: 10,
                    created_at: 1,
                },
                AgentRunTranscriptEntry {
                    run_id: AgentRunId::new(),
                    ordinal: 1,
                    role: TranscriptRole::Orchestrator,
                    kind: TranscriptKind::TerminalNote,
                    redacted_body: "the answer".to_string(),
                    byte_len: 9,
                    created_at: 2,
                },
            ],
            has_more: false,
            truncated: false,
            total_bytes: 19,
            total_entries: 2,
        };
        let context = build_seeded_context(&page);
        assert_eq!(
            context,
            "orchestrator/model_message: the prompt\norchestrator/terminal_note: the answer"
        );
    }

    #[test]
    fn a_truncated_transcript_says_it_is_truncated() {
        let page = AgentRunTranscriptPage {
            entries: vec![AgentRunTranscriptEntry {
                run_id: AgentRunId::new(),
                ordinal: 0,
                role: TranscriptRole::Orchestrator,
                kind: TranscriptKind::ModelMessage,
                redacted_body: "the prompt".to_string(),
                byte_len: 10,
                created_at: 1,
            }],
            has_more: false,
            truncated: true,
            total_bytes: 10,
            total_entries: 1,
        };
        let context = build_seeded_context(&page);
        assert!(
            context.contains("truncated"),
            "a truncated transcript must say so, got: {context}"
        );
    }

    fn claim(run_id: &AgentRunId, ordinal: i64) -> NewContinuationClaim {
        NewContinuationClaim {
            run_id: run_id.clone(),
            attempt_ordinal: ordinal,
            attempt_kind: AgentRunAttemptKind::Continuation,
            continued_from_ordinal: Some(1),
            principal: "parent-session".to_string(),
            principal_turn_id: "turn".to_string(),
            authority_digest: "authority".to_string(),
            transcript_digest: Some("digest".to_string()),
            failure_class: DelegatedFailureClass::ProviderRateLimited,
        }
    }

    #[tokio::test]
    async fn exactly_one_claim_wins_and_the_duplicate_is_refused() {
        let store = InMemoryAgentRunAttemptStore::new();
        let run_id = AgentRunId::new();
        let first = store
            .claim_continuation(claim(&run_id, 2))
            .await
            .expect("the first claim");
        assert!(matches!(first, ContinuationClaimOutcome::Claimed(_)));
        let second = store
            .claim_continuation(claim(&run_id, 2))
            .await
            .expect("the duplicate claim is answered, not an error");
        assert!(
            matches!(second, ContinuationClaimOutcome::AlreadyClaimed(_)),
            "the duplicate must be refused, not replayed"
        );
        let claims = store.continuations().await.expect("the continuation rows");
        assert_eq!(
            claims.len(),
            1,
            "exactly one continuation may be claimed, got: {claims:?}"
        );
        let attempts = store.attempts(&run_id).await.expect("the attempt rows");
        assert!(
            attempts.is_empty(),
            "the claim itself creates no attempt row; the attempt row is written at dispatch: \
             {attempts:?}"
        );
    }

    #[tokio::test]
    async fn a_released_claim_leaves_the_continuation_available() {
        let store = InMemoryAgentRunAttemptStore::new();
        let run_id = AgentRunId::new();
        store
            .claim_continuation(claim(&run_id, 2))
            .await
            .expect("the claim");
        assert!(store.release_claim(&run_id, 2).await.expect("release"));
        assert!(
            store.continuation(&run_id).await.expect("read").is_none(),
            "a released claim must leave no authorization behind"
        );
    }

    #[tokio::test]
    async fn a_run_with_no_attempt_record_reads_as_attempt_one() {
        let store = InMemoryAgentRunAttemptStore::new();
        let run_id = AgentRunId::new();
        assert_eq!(
            store.attempt_ordinal(&run_id).await.expect("the ordinal"),
            1,
            "a run recorded before the attempt record reads as its first attempt"
        );
    }
}

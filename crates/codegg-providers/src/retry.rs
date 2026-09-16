//! Unified retry budget and side-effect reconciliation (M002).
//!
//! One bounded retry chain propagated through provider, tool, scheduler,
//! and compatible external-effect paths. Lower layers may consume but never
//! replenish the parent budget. When an operation may have committed but its
//! acknowledgement was lost, it is classified as an uncertain side effect
//! and must be reconciled against canonical state before any retry.
//!
//! `std::time::Instant` never crosses a durable/protocol boundary; use
//! [`RetryContextDto`] for transport.

use serde::{Deserialize, Serialize};
use std::time::{Duration, Instant};

/// Hard ceiling for any single retry chain. Provider caps (3) and tool caps
/// remain lower ceilings; this bound prevents multiplicative nesting.
pub const MAX_CHAIN_ATTEMPTS: u8 = 8;

/// Hard ceiling for chain wall-clock duration.
pub const MAX_CHAIN_DURATION: Duration = Duration::from_secs(300);

/// Upper bound for human-readable uncertain-effect diagnostics. Details are
/// truncated and must already be secret-safe at the call site.
pub const MAX_UNCERTAIN_DETAIL_CHARS: usize = 500;

/// Opaque retry-chain identity, stable for exactly one logical operation.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct RetryChainId(String);

impl RetryChainId {
    pub fn new() -> Self {
        Self(uuid::Uuid::new_v4().to_string())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Default for RetryChainId {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Display for RetryChainId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// In-process bounded retry budget. Created at the logical operation/turn
/// boundary and passed downward; nested operations derive narrower children.
#[derive(Debug, Clone)]
pub struct RetryContext {
    chain_id: RetryChainId,
    total_attempts: u8,
    attempts_remaining: u8,
    deadline: Option<Instant>,
    created_at: Instant,
}

impl RetryContext {
    /// Create a chain with `total_attempts` total tries (including the first)
    /// and a wall-clock `timeout`. Values are clamped to the hard ceilings.
    pub fn new(total_attempts: u8, timeout: Duration) -> Self {
        let total = total_attempts.clamp(1, MAX_CHAIN_ATTEMPTS);
        let timeout = timeout.min(MAX_CHAIN_DURATION);
        let now = Instant::now();
        Self {
            chain_id: RetryChainId::new(),
            total_attempts: total,
            attempts_remaining: total,
            deadline: Some(now + timeout),
            created_at: now,
        }
    }

    /// Conservative default for legacy callers: a single attempt with a
    /// 60s deadline. Preserves current single-attempt semantics.
    pub fn single_attempt() -> Self {
        Self::new(1, Duration::from_secs(60))
    }

    /// Default chain for a provider turn when the caller supplies none.
    /// Matches the M001 lower ceiling (3 attempts, 120s setup budget).
    pub fn for_provider_turn() -> Self {
        Self::new(3, Duration::from_secs(120))
    }

    /// Default chain for a top-level logical operation spanning provider +
    /// tool retries. Provider (3) and tool (3) ceilings compose under it.
    pub fn for_operation() -> Self {
        Self::new(6, Duration::from_secs(180))
    }

    pub fn chain_id(&self) -> &RetryChainId {
        &self.chain_id
    }

    pub fn total_attempts(&self) -> u8 {
        self.total_attempts
    }

    pub fn attempts_remaining(&self) -> u8 {
        self.attempts_remaining
    }

    pub fn attempts_consumed(&self) -> u8 {
        self.total_attempts.saturating_sub(self.attempts_remaining)
    }

    pub fn is_exhausted(&self) -> bool {
        self.attempts_remaining == 0
    }

    pub fn is_expired(&self) -> bool {
        self.deadline.is_some_and(|d| Instant::now() >= d)
    }

    pub fn is_live(&self) -> bool {
        !self.is_exhausted() && !self.is_expired()
    }

    pub fn remaining_time(&self) -> Option<Duration> {
        self.deadline
            .map(|d| d.saturating_duration_since(Instant::now()))
    }

    /// Consume one attempt. Returns `false` when the budget is exhausted;
    /// the caller must stop retrying and return the last typed failure.
    pub fn consume_one(&mut self) -> bool {
        if self.attempts_remaining == 0 {
            return false;
        }
        self.attempts_remaining -= 1;
        true
    }

    /// Derive a narrower child for a nested layer. The child shares the
    /// chain ID and deadline but can never hold more attempts than the
    /// parent has remaining.
    pub fn derive_child(&self, max_attempts: u8) -> Self {
        let capped = max_attempts.clamp(1, MAX_CHAIN_ATTEMPTS);
        let remaining = capped.min(self.attempts_remaining);
        Self {
            chain_id: self.chain_id.clone(),
            total_attempts: remaining,
            attempts_remaining: remaining,
            deadline: self.deadline,
            created_at: self.created_at,
        }
    }

    /// Project to a transport-safe DTO (no `Instant`).
    pub fn to_dto(&self) -> RetryContextDto {
        RetryContextDto {
            chain_id: self.chain_id.as_str().to_string(),
            total_attempts: self.total_attempts,
            attempts_remaining: self.attempts_remaining,
            remaining_millis: self
                .remaining_time()
                .map(|d| d.as_millis().min(u64::MAX as u128) as u64),
        }
    }

    /// Rebuild an in-process context from a DTO, with a fresh deadline
    /// derived from the transported remaining time. Never restores more
    /// attempts than the DTO carries.
    pub fn from_dto(dto: &RetryContextDto) -> Self {
        let total = dto.total_attempts.clamp(1, MAX_CHAIN_ATTEMPTS);
        let remaining = dto.attempts_remaining.min(total);
        let now = Instant::now();
        let deadline = dto.remaining_millis.map(|ms| {
            let capped = Duration::from_millis(ms).min(MAX_CHAIN_DURATION);
            now + capped
        });
        Self {
            chain_id: RetryChainId(dto.chain_id.clone()),
            total_attempts: total,
            attempts_remaining: remaining,
            deadline,
            created_at: now,
        }
    }
}

/// Transport-safe retry-chain snapshot. Carries durations, never `Instant`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RetryContextDto {
    pub chain_id: String,
    pub total_attempts: u8,
    pub attempts_remaining: u8,
    pub remaining_millis: Option<u64>,
}

/// Dispatch/acknowledgement state for an operation that may have committed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AckState {
    /// Never left the caller (validation/permission/deadline failure).
    NotDispatched,
    /// Failed before any commit was possible (e.g. connection refused
    /// during setup, schema rejection from a dry-run check).
    FailedBeforeCommit,
    /// The backend acknowledged a terminal outcome (success or explicit
    /// failure). Safe to classify without reconciliation.
    Acknowledged,
    /// The operation may have committed but acknowledgement was lost
    /// (timeout, connection reset, stream cut after dispatch). Never
    /// automatically replayed for non-idempotent effects.
    Uncertain,
}

impl AckState {
    /// Whether a retry is even thinkable before effect-class gating.
    /// Only `Uncertain` forces the reconciliation path; the rest are
    /// decided by the effect/idempotency matrix.
    pub fn requires_reconciliation(self) -> bool {
        matches!(self, Self::Uncertain)
    }
}

/// Unified retry disposition across provider/tool/scheduler layers.
///
/// This composes the M001 provider taxonomy (`Permanent`/`Transient`/
/// `Conditional`) with tool/scheduler outcomes. `UncertainSideEffect`
/// always wins over `Transient`: an ambiguous commit is reconciled or
/// surfaced, never blindly replayed because the transport looked retryable.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UnifiedRetryDisposition {
    Never,
    Transient,
    RateLimited { retry_after: Option<Duration> },
    AuthRefreshable,
    UncertainSideEffect,
}

impl UnifiedRetryDisposition {
    /// Map the canonical M001 provider disposition into the chain.
    pub fn from_provider(disposition: crate::error::RetryDisposition) -> Self {
        match disposition {
            crate::error::RetryDisposition::Transient => Self::Transient,
            crate::error::RetryDisposition::Permanent => Self::Never,
            crate::error::RetryDisposition::Conditional => Self::AuthRefreshable,
        }
    }

    pub fn is_retryable(&self) -> bool {
        matches!(
            self,
            Self::Transient | Self::RateLimited { .. } | Self::AuthRefreshable
        )
    }
}

/// Typed uncertain side effect. Returned instead of replaying when safe
/// reconciliation is unavailable. All fields are bounded and secret-safe;
/// `detail` is truncated at construction and must not contain credentials,
/// URLs with keys, or command secrets (callers pass redacted text).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UncertainSideEffect {
    pub chain_id: String,
    pub domain: String,
    pub operation: String,
    pub invocation_key: Option<String>,
    pub detail: String,
}

impl UncertainSideEffect {
    pub fn new(
        chain_id: &str,
        domain: &str,
        operation: &str,
        invocation_key: Option<String>,
        detail: &str,
    ) -> Self {
        let detail: String = detail.chars().take(MAX_UNCERTAIN_DETAIL_CHARS).collect();
        Self {
            chain_id: chain_id.to_string(),
            domain: domain.to_string(),
            operation: operation.to_string(),
            invocation_key,
            detail,
        }
    }

    /// Short secret-safe summary for tracing/metrics (no payload).
    pub fn summary(&self) -> String {
        format!(
            "uncertain {}:{} chain={} key={}",
            self.domain,
            self.operation,
            self.chain_id,
            self.invocation_key.as_deref().unwrap_or("-")
        )
    }
}

impl std::fmt::Display for UncertainSideEffect {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "uncertain side effect ({}:{} chain={}): {}",
            self.domain, self.operation, self.chain_id, self.detail
        )
    }
}

impl std::error::Error for UncertainSideEffect {}

/// Outcome of a narrow per-domain reconciliation attempt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReconciliationOutcome {
    /// Canonical state proves the effect landed (or provably did not);
    /// the caller may treat the result as acknowledged.
    Reconciled { detail: String },
    /// Canonical state is unavailable or inconclusive; the caller must
    /// surface uncertainty instead of replaying.
    Unreconciled(UncertainSideEffect),
    /// This domain has no reconciler; the caller decides via the
    /// effect matrix (safe reads may retry, mutations go uncertain).
    NotApplicable,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn budget_consumption_is_bounded() {
        let mut ctx = RetryContext::new(3, Duration::from_secs(60));
        assert_eq!(ctx.attempts_remaining(), 3);
        assert!(ctx.is_live());
        assert!(ctx.consume_one());
        assert!(ctx.consume_one());
        assert!(ctx.consume_one());
        assert!(ctx.is_exhausted());
        assert!(!ctx.is_live());
        assert!(!ctx.consume_one());
        assert_eq!(ctx.attempts_consumed(), 3);
    }

    #[test]
    fn chain_clamps_to_hard_ceiling() {
        let ctx = RetryContext::new(255, Duration::from_secs(3600));
        assert_eq!(ctx.total_attempts(), MAX_CHAIN_ATTEMPTS);
        assert!(ctx.remaining_time().unwrap() <= MAX_CHAIN_DURATION);
    }

    #[test]
    fn nested_child_cannot_replenish_parent() {
        let mut parent = RetryContext::new(2, Duration::from_secs(60));
        assert!(parent.consume_one());
        assert_eq!(parent.attempts_remaining(), 1);
        let child = parent.derive_child(8);
        assert_eq!(child.attempts_remaining(), 1);
        assert_eq!(child.chain_id(), parent.chain_id());
        // Requesting fewer is honored; requesting more is capped.
        let narrow = parent.derive_child(1);
        assert_eq!(narrow.attempts_remaining(), 1);
        let mut fresh = RetryContext::new(4, Duration::from_secs(60));
        let wide_request = fresh.derive_child(2);
        assert_eq!(wide_request.attempts_remaining(), 2);
        assert!(fresh.consume_one());
        let _ = wide_request;
    }

    #[test]
    fn deadline_expiry_stops_chain() {
        let mut ctx = RetryContext::new(5, Duration::from_millis(1));
        std::thread::sleep(Duration::from_millis(5));
        assert!(ctx.is_expired());
        assert!(!ctx.is_live());
        // Budget remains but the deadline wins.
        assert_eq!(ctx.attempts_remaining(), 5);
        let _ = ctx.consume_one();
    }

    #[test]
    fn dto_round_trip_never_increases_budget() {
        let mut ctx = RetryContext::new(4, Duration::from_secs(60));
        assert!(ctx.consume_one());
        let dto = ctx.to_dto();
        assert_eq!(dto.attempts_remaining, 3);
        assert!(dto.remaining_millis.is_some());
        let restored = RetryContext::from_dto(&dto);
        assert_eq!(restored.chain_id().as_str(), ctx.chain_id().as_str());
        assert_eq!(restored.attempts_remaining(), 3);
        // A tampered DTO claiming more than the total is clamped.
        let mut tampered = dto.clone();
        tampered.attempts_remaining = 250;
        let clamped = RetryContext::from_dto(&tampered);
        assert!(clamped.attempts_remaining() <= clamped.total_attempts());
    }

    #[test]
    fn ack_state_reconciliation_gate() {
        assert!(!AckState::NotDispatched.requires_reconciliation());
        assert!(!AckState::FailedBeforeCommit.requires_reconciliation());
        assert!(!AckState::Acknowledged.requires_reconciliation());
        assert!(AckState::Uncertain.requires_reconciliation());
    }

    #[test]
    fn provider_disposition_mapping() {
        use crate::error::RetryDisposition as P;
        assert_eq!(
            UnifiedRetryDisposition::from_provider(P::Transient),
            UnifiedRetryDisposition::Transient
        );
        assert_eq!(
            UnifiedRetryDisposition::from_provider(P::Permanent),
            UnifiedRetryDisposition::Never
        );
        assert_eq!(
            UnifiedRetryDisposition::from_provider(P::Conditional),
            UnifiedRetryDisposition::AuthRefreshable
        );
        assert!(UnifiedRetryDisposition::Transient.is_retryable());
        assert!(!UnifiedRetryDisposition::Never.is_retryable());
        assert!(!UnifiedRetryDisposition::UncertainSideEffect.is_retryable());
    }

    #[test]
    fn uncertain_detail_is_bounded() {
        let long = "x".repeat(5000);
        let u = UncertainSideEffect::new("c1", "tool", "bash", None, &long);
        assert!(u.detail.len() <= MAX_UNCERTAIN_DETAIL_CHARS);
        assert_eq!(u.domain, "tool");
        assert!(u.summary().contains("c1"));
    }
}

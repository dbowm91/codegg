//! Unified retry budget and side-effect reconciliation for tools (M002).
//!
//! One bounded [`RetryContext`](crate::provider::RetryContext) chain spans
//! provider, tool, scheduler, and external-effect retries. Lower layers
//! consume but never replenish the parent budget. When an operation may have
//! committed but acknowledgement was lost, it becomes a typed
//! [`UncertainSideEffect`](crate::provider::UncertainSideEffect) instead of
//! being blindly replayed.
//!
//! Retry never grants new permissions and never alters sandbox, model, or
//! provider selection. Diagnostics carry chain/disposition metadata only;
//! tool arguments, credentials, URLs, and command bodies are never logged.

use crate::provider::{AckState, ReconciliationOutcome, RetryContext, UncertainSideEffect};
use crate::tool::contract::{IdempotencyClass, ToolContract, ToolEffectClass};
use codegg_core::error::ToolError;

/// Decision for a single tool execution failure under the shared chain.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ToolRetryDecision {
    /// Safe to retry within the remaining chain budget.
    Retry { attempts_left: u8 },
    /// Must not retry (permanent, exhausted, or unsafe).
    DoNotRetry { reason: &'static str },
    /// May have committed; reconcile or surface instead of replaying.
    Uncertain(UncertainSideEffect),
}

/// Classify a tool failure into dispatch/acknowledgement state.
///
/// - Failures before the backend is invoked (validation, deadline precheck)
///   are [`AckState::NotDispatched`].
/// - Explicit backend rejections (`Permission`, `Disabled`, `NotFound`,
///   `Format`, `Execution`) are [`AckState::Acknowledged`]: the backend
///   answered, so there is no commit ambiguity.
/// - Transport-class failures after dispatch (`Timeout`, `Network`, `Io`)
///   are [`AckState::Uncertain`]: the mutation may have landed.
pub fn ack_for_dispatch_phase(dispatched: bool, error: &ToolError) -> AckState {
    if !dispatched {
        return AckState::NotDispatched;
    }
    match error {
        ToolError::Timeout(_) | ToolError::Network(_) | ToolError::Io(_) => AckState::Uncertain,
        _ => AckState::Acknowledged,
    }
}

/// Whether the effect class may ever retry after a commit is possible.
/// Mirrors `ToolEffectClass::is_retry_eligible` but keeps the M002 matrix
/// explicit at the call site.
pub fn is_effect_retry_eligible(effect: ToolEffectClass) -> bool {
    effect.is_retry_eligible()
}

/// Decide whether a failed tool call may retry under the shared chain.
///
/// `invocation_key` is the stable submission/invocation identity (broker
/// `submission_key` / backend `invocation_key`). `IdempotentMutating`
/// retries require it; `NonIdempotent`/`ProcessExec` never auto-retry once
/// dispatch may have occurred.
pub fn decide_tool_retry(
    contract: &ToolContract,
    ack: AckState,
    invocation_key: Option<&str>,
    error: &ToolError,
    chain: &RetryContext,
) -> ToolRetryDecision {
    // Permission/authority failures are never retryable and must not
    // consume the shared budget as if they were transient.
    if matches!(
        error,
        ToolError::Permission(_) | ToolError::Disabled(_) | ToolError::NotFound(_)
    ) {
        return ToolRetryDecision::DoNotRetry {
            reason: "permanent denial; never retry",
        };
    }
    // Validation/format failures are permanent for the unchanged input.
    if matches!(error, ToolError::Format(_)) {
        return ToolRetryDecision::DoNotRetry {
            reason: "permanent validation failure",
        };
    }
    // Only transport-class errors are retry candidates at all.
    if !error.is_retryable() {
        return ToolRetryDecision::DoNotRetry {
            reason: "non-retryable execution error",
        };
    }
    if chain.is_expired() {
        return ToolRetryDecision::DoNotRetry {
            reason: "retry chain deadline expired",
        };
    }
    if chain.is_exhausted() {
        return ToolRetryDecision::DoNotRetry {
            reason: "retry chain budget exhausted",
        };
    }

    let has_key = invocation_key.is_some_and(|k| !k.is_empty());
    let contract_allows_retry = contract.retry_policy.max_retries > 0;

    // Ambiguous non-idempotent effects surface as uncertain even when the
    // contract disables blind retry: uncertainty is not a retry, it is the
    // safe alternative to replay. This check precedes the max_retries gate
    // so legacy NonIdempotent/ProcessExec contracts (max_retries=0) still
    // produce a typed uncertain result instead of a misleading timeout.
    if ack.requires_reconciliation() {
        match contract.effect_class {
            ToolEffectClass::NonIdempotent | ToolEffectClass::ProcessExec => {
                return ToolRetryDecision::Uncertain(uncertain_for_tool(
                    chain,
                    &contract.name,
                    effect_operation_label(contract.effect_class),
                    invocation_key,
                    "non-idempotent or process effect may have committed; \
                     refusing automatic replay",
                ));
            }
            ToolEffectClass::IdempotentMutating if !has_key => {
                return ToolRetryDecision::Uncertain(uncertain_for_tool(
                    chain,
                    &contract.name,
                    "idempotent_mutating_without_key",
                    invocation_key,
                    "idempotent mutation lost acknowledgement without a stable \
                     invocation key; refusing blind replay",
                ));
            }
            _ => {}
        }
    }

    if !contract_allows_retry {
        return ToolRetryDecision::DoNotRetry {
            reason: "tool contract disables retry",
        };
    }

    match contract.effect_class {
        ToolEffectClass::ReadOnly | ToolEffectClass::ReadValidate | ToolEffectClass::SafeRepeat => {
            ToolRetryDecision::Retry {
                attempts_left: chain.attempts_remaining(),
            }
        }
        ToolEffectClass::IdempotentMutating => {
            // Idempotent overwrite/deduplication is only safe when the
            // same stable key is reused so the backend converges.
            if has_key {
                ToolRetryDecision::Retry {
                    attempts_left: chain.attempts_remaining(),
                }
            } else if ack.requires_reconciliation() {
                ToolRetryDecision::Uncertain(uncertain_for_tool(
                    chain,
                    &contract.name,
                    "idempotent_mutating_without_key",
                    invocation_key,
                    "idempotent mutation lost acknowledgement without a stable \
                     invocation key; refusing blind replay",
                ))
            } else {
                ToolRetryDecision::DoNotRetry {
                    reason: "idempotent mutation requires a stable invocation key",
                }
            }
        }
        ToolEffectClass::NonIdempotent | ToolEffectClass::ProcessExec => {
            // Before dispatch nothing committed, so a bounded retry is
            // safe. After dispatch (acknowledged failure or uncertain
            // loss), never auto-replay: reconcile or surface.
            match ack {
                AckState::NotDispatched | AckState::FailedBeforeCommit => {
                    ToolRetryDecision::Retry {
                        attempts_left: chain.attempts_remaining(),
                    }
                }
                AckState::Acknowledged | AckState::Uncertain => {
                    ToolRetryDecision::Uncertain(uncertain_for_tool(
                        chain,
                        &contract.name,
                        effect_operation_label(contract.effect_class),
                        invocation_key,
                        "non-idempotent or process effect may have committed; \
                         refusing automatic replay",
                    ))
                }
            }
        }
    }
}

/// Decide retry for a job-idempotency class (scheduler-owned durable work).
/// Only `ReadOnly`/`SafeRepeat` auto-retry; `Conditional` retries solely via
/// explicit reconciliation; `NonIdempotent`/`Destructive` never auto-retry.
pub fn decide_job_retry(
    idempotency: codegg_core::jobs::IdempotencyClass,
    ack: AckState,
    chain: &RetryContext,
    tool_name: &str,
) -> ToolRetryDecision {
    if chain.is_expired() || chain.is_exhausted() {
        return ToolRetryDecision::DoNotRetry {
            reason: "retry chain exhausted or expired",
        };
    }
    match idempotency {
        codegg_core::jobs::IdempotencyClass::ReadOnly
        | codegg_core::jobs::IdempotencyClass::SafeRepeat => ToolRetryDecision::Retry {
            attempts_left: chain.attempts_remaining(),
        },
        _ => {
            if matches!(ack, AckState::NotDispatched | AckState::FailedBeforeCommit) {
                ToolRetryDecision::Retry {
                    attempts_left: chain.attempts_remaining(),
                }
            } else {
                ToolRetryDecision::Uncertain(uncertain_for_tool(
                    chain,
                    tool_name,
                    "durable_job",
                    None,
                    "durable job may have been accepted; reconcile by submission \
                     key before resubmitting",
                ))
            }
        }
    }
}

/// Build a secret-safe uncertain effect for a tool. Never includes tool
/// arguments, command bodies, URLs, or credentials — only identities and a
/// bounded operator-facing reason.
pub fn uncertain_for_tool(
    chain: &RetryContext,
    tool_name: &str,
    operation: &str,
    invocation_key: Option<&str>,
    reason: &str,
) -> UncertainSideEffect {
    UncertainSideEffect::new(
        chain.chain_id().as_str(),
        "tool",
        &format!("{tool_name}:{operation}"),
        invocation_key.map(str::to_string),
        reason,
    )
}

/// Narrow per-domain reconciliation for Git operations.
///
/// Read-only subcommands need no reconciliation (`NotApplicable`).
/// Mutations with lost acknowledgement return `Unreconciled`: the caller
/// must inspect the exact remote/ref expected result through the existing
/// Git service before any retry, and must surface uncertainty when that
/// inspection is unavailable. This helper never shells out; it only
/// classifies so the Git tool can route to its existing read paths.
pub fn reconcile_git_operation(
    subcommand: &str,
    ack: AckState,
    chain: &RetryContext,
    invocation_key: Option<&str>,
) -> ReconciliationOutcome {
    const READ_ONLY: &[&str] = &[
        "status",
        "diff",
        "log",
        "show",
        "blame",
        "branch",
        "tag",
        "remote",
        "worktree",
        "stash",
        "rev-parse",
        "for-each-ref",
        "fetch",
    ];
    if READ_ONLY.contains(&subcommand) {
        return ReconciliationOutcome::NotApplicable;
    }
    if !ack.requires_reconciliation() {
        return ReconciliationOutcome::NotApplicable;
    }
    ReconciliationOutcome::Unreconciled(UncertainSideEffect::new(
        chain.chain_id().as_str(),
        "git",
        subcommand,
        invocation_key.map(str::to_string),
        "git mutation lost acknowledgement; inspect the exact remote/ref \
         expected result before any retry",
    ))
}

/// Reconciliation for idempotent external APIs (MCP/HTTP mutations) where
/// the contract defines an explicit get/status operation or the backend
/// deduplicates on a stable idempotency key.
///
/// Returns `Reconciled` only when the caller proves both a stable key and
/// backend dedup/status support; otherwise `Unreconciled`. Never invents
/// idempotency the API does not offer.
pub fn reconcile_idempotent_api(
    has_stable_key: bool,
    backend_supports_dedup: bool,
    chain: &RetryContext,
    operation: &str,
    invocation_key: Option<&str>,
) -> ReconciliationOutcome {
    if has_stable_key && backend_supports_dedup {
        ReconciliationOutcome::Reconciled {
            detail: format!("safe to retry {operation} with the same stable key"),
        }
    } else if !has_stable_key && !backend_supports_dedup {
        ReconciliationOutcome::NotApplicable
    } else {
        ReconciliationOutcome::Unreconciled(UncertainSideEffect::new(
            chain.chain_id().as_str(),
            "api",
            operation,
            invocation_key.map(str::to_string),
            "external mutation lost acknowledgement without proven idempotent \
             retry support; refusing blind replay",
        ))
    }
}

fn effect_operation_label(effect: ToolEffectClass) -> &'static str {
    match effect {
        ToolEffectClass::ReadOnly => "read_only",
        ToolEffectClass::ReadValidate => "read_validate",
        ToolEffectClass::SafeRepeat => "safe_repeat",
        ToolEffectClass::IdempotentMutating => "idempotent_mutating",
        ToolEffectClass::NonIdempotent => "non_idempotent",
        ToolEffectClass::ProcessExec => "process_exec",
    }
}

/// Validate that a contract's retry policy is compatible with the shared
/// budget. Rejects unsafe combinations (retry enabled on non-retryable
/// effects without a stable-key path) at registration time.
pub fn validate_retry_contract(
    effect: ToolEffectClass,
    idempotency: IdempotencyClass,
    max_retries: u8,
    has_stable_key_path: bool,
) -> Result<(), &'static str> {
    if max_retries == 0 {
        return Ok(());
    }
    match effect {
        ToolEffectClass::ReadOnly | ToolEffectClass::ReadValidate | ToolEffectClass::SafeRepeat => {
            Ok(())
        }
        ToolEffectClass::IdempotentMutating => {
            if has_stable_key_path || matches!(idempotency, IdempotencyClass::Idempotent) {
                Ok(())
            } else {
                Err("idempotent mutation enables retry only with a stable key path")
            }
        }
        ToolEffectClass::NonIdempotent | ToolEffectClass::ProcessExec => {
            Err("non-idempotent/process effects must not enable blind retry")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tool::contract::{ToolCachePolicy, ToolProjectionPolicy};
    use std::time::Duration;

    fn contract_for(
        effect: ToolEffectClass,
        idempotency: IdempotencyClass,
        max_retries: u8,
    ) -> ToolContract {
        ToolContract {
            name: "test_tool".to_string(),
            caller_policy: crate::tool::contract::ToolCallerPolicy::DirectOnly,
            effect_class: effect,
            idempotency,
            retry_policy: crate::tool::contract::ToolRetryPolicy {
                max_retries,
                base_delay_ms: 10,
                max_delay_ms: 100,
            },
            cache_policy: ToolCachePolicy::default(),
            projection_policy: ToolProjectionPolicy::default(),
            implementation_id: "codegg/test_tool".to_string(),
            implementation_version: "0.0.0".to_string(),
            input_schema: serde_json::json!({"type": "object"}),
            output_schema: None,
        }
    }

    fn chain_with(budget: u8) -> RetryContext {
        RetryContext::new(budget, Duration::from_secs(60))
    }

    #[test]
    fn effect_matrix_safe_reads_retry() {
        let chain = chain_with(3);
        for effect in [
            ToolEffectClass::ReadOnly,
            ToolEffectClass::ReadValidate,
            ToolEffectClass::SafeRepeat,
        ] {
            let c = contract_for(effect, IdempotencyClass::Idempotent, 2);
            let d = decide_tool_retry(
                &c,
                AckState::Uncertain,
                None,
                &ToolError::Timeout("t".into()),
                &chain,
            );
            assert!(matches!(d, ToolRetryDecision::Retry { .. }), "{effect:?}");
        }
    }

    #[test]
    fn idempotent_mutating_requires_stable_key() {
        let chain = chain_with(3);
        let c = contract_for(
            ToolEffectClass::IdempotentMutating,
            IdempotencyClass::Idempotent,
            2,
        );
        let with_key = decide_tool_retry(
            &c,
            AckState::Uncertain,
            Some("key-1"),
            &ToolError::Timeout("t".into()),
            &chain,
        );
        assert!(matches!(with_key, ToolRetryDecision::Retry { .. }));
        let without_key = decide_tool_retry(
            &c,
            AckState::Uncertain,
            None,
            &ToolError::Timeout("t".into()),
            &chain,
        );
        assert!(matches!(without_key, ToolRetryDecision::Uncertain(_)));
    }

    #[test]
    fn non_idempotent_dispatched_timeout_is_uncertain() {
        let chain = chain_with(3);
        let c = contract_for(
            ToolEffectClass::NonIdempotent,
            IdempotencyClass::NonIdempotent,
            2,
        );
        let d = decide_tool_retry(
            &c,
            AckState::Uncertain,
            Some("k"),
            &ToolError::Timeout("dispatched then hung".into()),
            &chain,
        );
        match d {
            ToolRetryDecision::Uncertain(u) => {
                assert_eq!(u.domain, "tool");
                assert!(!u.detail.contains("secret"));
            }
            other => panic!("expected uncertain, got {other:?}"),
        }
    }

    #[test]
    fn raw_shell_ambiguous_is_never_replayed() {
        let chain = chain_with(3);
        let c = contract_for(
            ToolEffectClass::ProcessExec,
            IdempotencyClass::NonIdempotent,
            2,
        );
        for ack in [AckState::Acknowledged, AckState::Uncertain] {
            let d = decide_tool_retry(&c, ack, Some("k"), &ToolError::Timeout("t".into()), &chain);
            assert!(
                matches!(d, ToolRetryDecision::Uncertain(_)),
                "process exec {ack:?} must be uncertain"
            );
        }
        // Before dispatch a bounded retry is safe.
        let before = decide_tool_retry(
            &c,
            AckState::NotDispatched,
            Some("k"),
            &ToolError::Timeout("t".into()),
            &chain,
        );
        assert!(matches!(before, ToolRetryDecision::Retry { .. }));
    }

    #[test]
    fn permission_denied_never_retries() {
        let chain = chain_with(5);
        let c = contract_for(ToolEffectClass::ReadOnly, IdempotencyClass::Idempotent, 3);
        let d = decide_tool_retry(
            &c,
            AckState::NotDispatched,
            None,
            &ToolError::Permission("denied".into()),
            &chain,
        );
        assert!(matches!(d, ToolRetryDecision::DoNotRetry { .. }));
    }

    #[test]
    fn exhausted_chain_stops_retry() {
        let mut chain = chain_with(1);
        assert!(chain.consume_one());
        let c = contract_for(ToolEffectClass::ReadOnly, IdempotencyClass::Idempotent, 3);
        let d = decide_tool_retry(
            &c,
            AckState::NotDispatched,
            None,
            &ToolError::Timeout("t".into()),
            &chain,
        );
        assert!(matches!(d, ToolRetryDecision::DoNotRetry { .. }));
    }

    #[test]
    fn ack_classification() {
        assert_eq!(
            ack_for_dispatch_phase(false, &ToolError::Timeout("t".into())),
            AckState::NotDispatched
        );
        assert_eq!(
            ack_for_dispatch_phase(true, &ToolError::Timeout("t".into())),
            AckState::Uncertain
        );
        assert_eq!(
            ack_for_dispatch_phase(true, &ToolError::Network("n".into())),
            AckState::Uncertain
        );
        assert_eq!(
            ack_for_dispatch_phase(true, &ToolError::Execution("e".into())),
            AckState::Acknowledged
        );
        assert_eq!(
            ack_for_dispatch_phase(true, &ToolError::Permission("p".into())),
            AckState::Acknowledged
        );
    }

    #[test]
    fn git_reconciliation_matrix() {
        let chain = chain_with(3);
        assert!(matches!(
            reconcile_git_operation("log", AckState::Uncertain, &chain, None),
            ReconciliationOutcome::NotApplicable
        ));
        assert!(matches!(
            reconcile_git_operation("push", AckState::NotDispatched, &chain, None),
            ReconciliationOutcome::NotApplicable
        ));
        match reconcile_git_operation("push", AckState::Uncertain, &chain, Some("k")) {
            ReconciliationOutcome::Unreconciled(u) => {
                assert_eq!(u.domain, "git");
                assert_eq!(u.operation, "push");
            }
            other => panic!("expected unreconciled, got {other:?}"),
        }
    }

    #[test]
    fn idempotent_api_reconciliation() {
        let chain = chain_with(3);
        assert!(matches!(
            reconcile_idempotent_api(true, true, &chain, "create", Some("k")),
            ReconciliationOutcome::Reconciled { .. }
        ));
        assert!(matches!(
            reconcile_idempotent_api(false, false, &chain, "create", None),
            ReconciliationOutcome::NotApplicable
        ));
        assert!(matches!(
            reconcile_idempotent_api(false, true, &chain, "create", None),
            ReconciliationOutcome::Unreconciled(_)
        ));
    }

    #[test]
    fn retry_contract_validation() {
        assert!(validate_retry_contract(
            ToolEffectClass::ReadOnly,
            IdempotencyClass::Idempotent,
            3,
            false
        )
        .is_ok());
        assert!(validate_retry_contract(
            ToolEffectClass::NonIdempotent,
            IdempotencyClass::NonIdempotent,
            1,
            true
        )
        .is_err());
        assert!(validate_retry_contract(
            ToolEffectClass::ProcessExec,
            IdempotencyClass::NonIdempotent,
            1,
            false
        )
        .is_err());
        assert!(validate_retry_contract(
            ToolEffectClass::IdempotentMutating,
            IdempotencyClass::NonIdempotent,
            1,
            false
        )
        .is_err());
    }

    #[test]
    fn uncertain_diagnostics_omit_payload() {
        let chain = chain_with(3);
        let u = uncertain_for_tool(
            &chain,
            "bash",
            "process_exec",
            Some("inv-1"),
            "may have committed",
        );
        // Only identities + bounded reason; no arguments smuggled in.
        assert!(!u.detail.contains("rm -rf"));
        assert_eq!(u.invocation_key.as_deref(), Some("inv-1"));
        assert_eq!(u.chain_id, chain.chain_id().as_str());
    }
}

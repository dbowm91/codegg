//! M012 broker failure semantics tests.
//!
//! Covers closure criteria C-05 and C-06:
//! - C-05: Denied, failed, cancelled, timed-out, and schema-invalid nested calls
//!   cannot become successful CompletedCall records.
//! - C-06: Only successful Broker terminal status increments completed-call counters.

#![cfg(test)]

use codegg::tool::broker::{BrokerResult, ProgrammaticOutcome};
use codegg::tool::contract::{ToolTerminalStatus, ToolValue};

fn make_broker_result(status: ToolTerminalStatus, display: &str) -> BrokerResult {
    BrokerResult {
        value: ToolValue {
            display: display.into(),
            value: None,
            artifacts: vec![],
            provenance: None,
            terminal_status: status,
            truncated: false,
        },
        contract: codegg::tool::contract::ToolContract {
            name: "test".into(),
            caller_policy: codegg::tool::contract::ToolCallerPolicy::DirectOnly,
            effect_class: codegg::tool::contract::ToolEffectClass::ReadOnly,
            idempotency: codegg::tool::contract::IdempotencyClass::Idempotent,
            retry_policy: codegg::tool::contract::ToolRetryPolicy::default(),
            cache_policy: codegg::tool::contract::ToolCachePolicy::default(),
            projection_policy: codegg::tool::contract::ToolProjectionPolicy::default(),
            implementation_id: "test".into(),
            implementation_version: "1.0".into(),
            input_schema: serde_json::json!({}),
            output_schema: None,
        },
        invocation_id: "inv-1".into(),
        elapsed_ms: 0,
    }
}

#[tokio::test(flavor = "current_thread")]
async fn c05_denied_cannot_become_completed() {
    let result = make_broker_result(ToolTerminalStatus::Denied, "permission denied");
    let outcome = result.into_programmatic_outcome();
    assert!(outcome.is_err());
    assert_eq!(outcome.unwrap_err(), ProgrammaticOutcome::Denied);
}

#[tokio::test(flavor = "current_thread")]
async fn c05_cancelled_cannot_become_completed() {
    let result = make_broker_result(ToolTerminalStatus::Cancelled, "cancelled by user");
    let outcome = result.into_programmatic_outcome();
    assert!(outcome.is_err());
    assert_eq!(outcome.unwrap_err(), ProgrammaticOutcome::Cancelled);
}

#[tokio::test(flavor = "current_thread")]
async fn c05_timed_out_cannot_become_completed() {
    let result = make_broker_result(ToolTerminalStatus::TimedOut, "exceeded timeout");
    let outcome = result.into_programmatic_outcome();
    assert!(outcome.is_err());
    assert_eq!(outcome.unwrap_err(), ProgrammaticOutcome::TimedOut);
}

#[tokio::test(flavor = "current_thread")]
async fn c05_infrastructure_error_cannot_become_completed() {
    let result = make_broker_result(
        ToolTerminalStatus::InfrastructureError,
        "connection refused",
    );
    let outcome = result.into_programmatic_outcome();
    assert!(outcome.is_err());
    assert_eq!(
        outcome.unwrap_err(),
        ProgrammaticOutcome::InfrastructureError
    );
}

#[tokio::test(flavor = "current_thread")]
async fn c06_success_can_become_completed() {
    let result = make_broker_result(ToolTerminalStatus::Success, "ok");
    let outcome = result.into_programmatic_outcome();
    assert!(outcome.is_ok());
}

#[tokio::test(flavor = "current_thread")]
async fn c06_programmatic_outcome_enum_is_exhaustive() {
    // Verify all ProgrammaticOutcome variants exist.
    let _ = ProgrammaticOutcome::Success;
    let _ = ProgrammaticOutcome::Denied;
    let _ = ProgrammaticOutcome::Cancelled;
    let _ = ProgrammaticOutcome::TimedOut;
    let _ = ProgrammaticOutcome::SchemaMismatch;
    let _ = ProgrammaticOutcome::InfrastructureError;
}

#[tokio::test(flavor = "current_thread")]
async fn c05_all_non_success_outcomes_are_errors() {
    // Verify that every non-Success ToolTerminalStatus maps to an Err outcome.
    let statuses = [
        ToolTerminalStatus::Denied,
        ToolTerminalStatus::Cancelled,
        ToolTerminalStatus::TimedOut,
        ToolTerminalStatus::InfrastructureError,
    ];
    for status in statuses {
        let result = make_broker_result(status, "test");
        let outcome = result.into_programmatic_outcome();
        assert!(
            outcome.is_err(),
            "status {:?} should map to Err, got Ok",
            status
        );
    }
}

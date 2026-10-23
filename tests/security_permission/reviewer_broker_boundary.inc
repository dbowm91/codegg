//! The permission reviewer's tool call: the recorded broker-boundary
//! exception, and the authority ceiling that exception must not change.
//!
//! The Automatic approval reviewer investigates an approval request with a
//! bounded read-only tool palette. That investigation reaches the tool
//! registry directly rather than the broker, and this is a **recorded
//! exception with a named owner**
//! (`scripts/check_tool_broker_boundary.py`; owner: the M006 Automatic
//! approval reviewer, `src/permission/reviewer.rs`,
//! `architecture/approval_reviewer.md`) — not an accident:
//!
//! - the broker refuses a caller that carries no verified authority grant,
//!   and the reviewer's grant would have to come from the very
//!   ApprovalRouter the reviewer is servicing;
//! - `scripts/check_approval_reviewer.py` already forbids the reviewer
//!   module from naming `ApprovalRouter` at all, so a broker-routed
//!   reviewer would break the reviewer's own isolation contract.
//!
//! This member pins both halves of that disposition: that the reviewer's
//! call is a registry dispatch the broker refuses before execution, and that
//! the ceiling it runs under is unchanged.

use codegg::error::ToolError;
use codegg::permission::reviewer::{
    RegistryReviewerInvestigator, ReviewerError, ReviewerInvestigator,
    REVIEWER_MAX_TOOL_OUTPUT_CHARS,
};
use codegg::tool::backend::{StructuredToolResult, ToolBackendKind, ToolExecutionContext};
use codegg::tool::broker::{BrokerError, BrokerInvocationContext, ToolBroker};
use codegg::tool::{Tool, ToolCategory, ToolRegistry};
use std::sync::{Arc, Mutex};

/// A read-only stand-in for the `read` tool that records the execution
/// context it was handed, so the reviewer's ceiling can be asserted from
/// outside the reviewer.
struct RecordingReadTool {
    seen: Arc<Mutex<Option<ToolExecutionContext>>>,
    payload: String,
}

#[async_trait::async_trait]
// Stable >=1.99 flags the #[must_use] injected by the async-trait expansion
// (the desugared future is already must_use). Suppressed at the macro site;
// no behavior change.
#[allow(clippy::double_must_use)]
impl Tool for RecordingReadTool {
    fn name(&self) -> &str {
        "read"
    }

    fn description(&self) -> &str {
        "records the execution context the reviewer built"
    }

    fn parameters(&self) -> serde_json::Value {
        serde_json::json!({"type": "object", "properties": {}})
    }

    fn category(&self) -> ToolCategory {
        ToolCategory::ReadOnly
    }

    async fn execute(&self, _input: serde_json::Value) -> Result<String, ToolError> {
        Ok(self.payload.clone())
    }

    async fn execute_structured(
        &self,
        input: serde_json::Value,
        ctx: Option<ToolExecutionContext>,
    ) -> Result<StructuredToolResult, ToolError> {
        *self.seen.lock().expect("recorder lock") = ctx;
        let output = self.execute(input).await?;
        Ok(StructuredToolResult::legacy(self.name(), output))
    }
}

/// A registry holding only the recording `read` tool.
fn registry_with_recorder(
    payload: &str,
) -> (ToolRegistry, Arc<Mutex<Option<ToolExecutionContext>>>) {
    let seen = Arc::new(Mutex::new(None));
    let mut registry = ToolRegistry::new();
    registry.register(RecordingReadTool {
        seen: Arc::clone(&seen),
        payload: payload.to_string(),
    });
    (registry, seen)
}

/// The reviewer's investigation reaches the registry and runs the tool; the
/// broker refuses the very same call, and refuses it *before* the tool runs.
///
/// This is the runtime shape of the recorded exception: the two routes are
/// not interchangeable, and the broker route is not available to a caller
/// that holds no authority grant.
#[tokio::test(flavor = "current_thread")]
async fn the_reviewers_call_is_a_registry_dispatch_the_broker_refuses() {
    let workspace = tempfile::tempdir().expect("workspace");
    let (registry, seen) = registry_with_recorder("reviewer saw the file");

    let investigator =
        RegistryReviewerInvestigator::new(&registry, workspace.path().to_path_buf(), "session-r1");
    let observed = investigator
        .investigate("read", &serde_json::json!({"path": "README.md"}))
        .await
        .expect("the reviewer must be able to investigate through the registry");
    assert_eq!(observed, "reviewer saw the file");
    assert!(
        seen.lock().expect("recorder lock").is_some(),
        "the reviewer's call must reach the tool"
    );

    // The same call through the broker is refused before execution: the
    // broker requires a verified authority grant, and the reviewer has none
    // to hold — it is adjudicating the request that would produce one.
    let (broker_registry, broker_seen) = registry_with_recorder("broker saw the file");
    let broker = ToolBroker::new(&broker_registry);
    let mut broker_ctx = ToolExecutionContext::with_backend(ToolBackendKind::Native);
    broker_ctx.cwd = workspace.path().to_path_buf();
    let broker_result = broker
        .execute(
            &broker_registry,
            "read",
            serde_json::json!({"path": "README.md"}),
            BrokerInvocationContext::from(broker_ctx),
        )
        .await;
    assert!(
        matches!(broker_result, Err(BrokerError::CallerDenied { .. })),
        "the broker must refuse a caller with no verified authority grant, got {broker_result:?}"
    );
    assert!(
        broker_seen.lock().expect("recorder lock").is_none(),
        "the broker must refuse before the tool runs"
    );
}

/// The reviewer's authority ceiling is unchanged: read-only effect class,
/// native-only backend policy, a 15-second timeout, and the reviewer's own
/// caller class and permission mode.
#[tokio::test(flavor = "current_thread")]
async fn the_reviewers_authority_ceiling_is_unchanged() {
    let workspace = tempfile::tempdir().expect("workspace");
    let (registry, seen) = registry_with_recorder("payload");

    let investigator =
        RegistryReviewerInvestigator::new(&registry, workspace.path().to_path_buf(), "session-r2");
    investigator
        .investigate("read", &serde_json::json!({"path": "README.md"}))
        .await
        .expect("investigation");

    let ctx = seen
        .lock()
        .expect("recorder lock")
        .clone()
        .expect("the reviewer must hand the tool an execution context");
    assert_eq!(
        ctx.max_effect_class.as_deref(),
        Some("read_only"),
        "the reviewer must run under a read-only effect class"
    );
    assert_eq!(
        ctx.backend_policy.as_deref(),
        Some("native_only"),
        "the reviewer must run under the native-only backend policy"
    );
    assert_eq!(
        ctx.timeout_ms,
        Some(15_000),
        "the reviewer's timeout must stay at 15 seconds"
    );
    assert_eq!(
        ctx.caller_class.as_deref(),
        Some("approval-reviewer"),
        "the reviewer's caller class must name the reviewer"
    );
    assert_eq!(
        ctx.permission_mode.as_deref(),
        Some("automatic-reviewer"),
        "the reviewer's permission mode must stay the reviewer's own"
    );
    assert_eq!(
        ctx.session_id.as_deref(),
        Some("session-r2"),
        "the reviewer's call must carry the reviewing session"
    );
    assert_eq!(
        ctx.backend,
        ToolBackendKind::Native,
        "the reviewer must run on the native backend"
    );
}

/// The reviewer's output ceiling is unchanged: the tool's output is
/// truncated to the reviewer's budget before it reaches the reviewer model.
#[tokio::test(flavor = "current_thread")]
async fn the_reviewers_output_is_truncated_to_its_budget() {
    let workspace = tempfile::tempdir().expect("workspace");
    let oversized = "x".repeat(REVIEWER_MAX_TOOL_OUTPUT_CHARS * 2);
    let (registry, _seen) = registry_with_recorder(&oversized);

    let investigator =
        RegistryReviewerInvestigator::new(&registry, workspace.path().to_path_buf(), "session-r3");
    let observed = investigator
        .investigate("read", &serde_json::json!({"path": "README.md"}))
        .await
        .expect("investigation");
    assert_eq!(
        observed.len(),
        REVIEWER_MAX_TOOL_OUTPUT_CHARS,
        "the reviewer must truncate the tool output to its budget"
    );
}

/// A tool outside the reviewer's palette is refused before any execution
/// context is built, so a refused tool cannot have run.
#[tokio::test(flavor = "current_thread")]
async fn the_reviewer_refuses_a_tool_outside_its_palette() {
    let workspace = tempfile::tempdir().expect("workspace");
    let (registry, seen) = registry_with_recorder("payload");

    let investigator =
        RegistryReviewerInvestigator::new(&registry, workspace.path().to_path_buf(), "session-r4");
    let err = investigator
        .investigate("bash", &serde_json::json!({"command": "rm -rf /"}))
        .await
        .expect_err("a non-allowlisted tool must be refused");
    assert!(
        matches!(err, ReviewerError::ForbiddenTool(_)),
        "the reviewer must refuse a non-allowlisted tool, got {err:?}"
    );
    assert!(
        seen.lock().expect("recorder lock").is_none(),
        "a refused tool must not have run"
    );
}

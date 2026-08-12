//! Typed tool-batch execution boundary for the turn driver.

use super::r#loop::AgentLoop;
use crate::agent::progress_recovery::ToolExecutionOutcome;
use crate::error::AppError;
use crate::provider::ToolCall;

/// Owns the batch boundary between provider turns and tool execution.
pub(super) struct ToolBatchExecutor<'a> {
    loop_: &'a mut AgentLoop,
}

impl<'a> ToolBatchExecutor<'a> {
    pub(super) fn new(loop_: &'a mut AgentLoop) -> Self {
        Self { loop_ }
    }

    pub(super) async fn execute(
        self,
        tool_calls: &[ToolCall],
    ) -> Result<Vec<(String, ToolExecutionOutcome)>, AppError> {
        self.loop_.execute_tool_calls_impl(tool_calls).await
    }
}

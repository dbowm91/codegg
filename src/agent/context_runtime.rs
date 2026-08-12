//! Context-policy runtime state owned separately from turn orchestration.

/// Ephemeral backoff/starvation state for context policy decisions.
///
/// This state is deliberately per-loop and non-persistent. Durable context
/// identity and usage statistics remain owned by the context subsystem.
#[derive(Debug, Clone, Default)]
pub(super) struct ContextPolicyRuntimeState {
    pub(super) reduction_disabled_until_turn: Option<usize>,
    pub(super) consecutive_reductions: usize,
    pub(super) last_selected_tool_count: usize,
    pub(super) last_omitted_tools: Vec<String>,
    pub(super) last_reason: Option<String>,
    pub(super) last_selected_tools: Vec<String>,
}

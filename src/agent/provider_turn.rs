//! Provider-turn normalization boundary.

use super::r#loop::AgentLoop;
use crate::error::AppError;
use crate::provider::{ChatEvent, ChatRequest};

/// Adapter-owned entry point for provider streaming, retries, and normalized
/// chat events. The turn driver does not need to know wire compatibility
/// details; it consumes the canonical event stream.
pub(super) struct ProviderTurnAdapter;

impl ProviderTurnAdapter {
    pub(super) async fn receive(
        loop_: &mut AgentLoop,
        request: &ChatRequest,
    ) -> Result<Vec<ChatEvent>, AppError> {
        loop_.stream_with_retry_impl(request).await
    }
}

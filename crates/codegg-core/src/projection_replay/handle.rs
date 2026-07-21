use std::sync::Arc;

use codegg_protocol::core::{CoreEvent, EventEnvelope};

use crate::error::StorageError;
use crate::projection_replay::metrics::ProjectionReplayMetricsSnapshot;
use crate::projection_replay::service::{ProjectionReplayService, PublishOutcome};

#[derive(Clone)]
pub struct ProjectionReplayHandle {
    inner: Arc<ProjectionReplayService>,
}

impl ProjectionReplayHandle {
    pub fn new(service: Arc<ProjectionReplayService>) -> Self {
        Self { inner: service }
    }

    pub async fn publish_core_event(
        &self,
        envelope: &EventEnvelope<CoreEvent>,
    ) -> Result<PublishOutcome, StorageError> {
        self.inner.publish_from_core(envelope).await
    }

    pub fn metrics_snapshot(&self) -> ProjectionReplayMetricsSnapshot {
        self.inner.metrics_snapshot()
    }
}

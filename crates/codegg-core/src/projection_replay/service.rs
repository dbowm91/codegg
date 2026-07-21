use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::Mutex;

use codegg_protocol::core::{CoreEvent, EventEnvelope};
use codegg_protocol::projection::event::ProjectionEnvelope;
use codegg_protocol::projection::replay::{
    ProjectionCursor, ProjectionResyncReason, ProjectionSnapshotBundle, ProjectionStreamDescriptor,
    ProjectionStreamKind, ProjectionSubscriptionId, ProjectionSubscriptionRequest,
};

use crate::error::StorageError;
use crate::projection_replay::metrics::ProjectionReplayMetrics;
use crate::projection_replay::publication::projection_events_from_core;
use crate::projection_replay::retention::RetentionPolicy;
use crate::projection_replay::seam::ProjectionPublicationContext;
use crate::projection_replay::store::ProjectionReplayStore;
use crate::projection_replay::subscription::{SubscriptionConfig, SubscriptionRegistry};

pub const MAX_REPLAY_EVENTS: usize = 512;
pub const MAX_REPLAY_BYTES: u64 = 1024 * 1024;

#[derive(Debug, Clone)]
pub enum PublishOutcome {
    Published {
        session_stream_seq: u64,
        project_stream_seq: u64,
    },
    Skipped {
        reason: SafePublicationReason,
    },
    Failed {
        error: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SafePublicationReason {
    UnboundSession,
    InternalEvent,
    ClientLocalWithoutOrigin,
    SensitiveRedacted,
    AdaptionEmpty,
}

#[derive(Debug, Clone)]
pub enum ResumeOutcome {
    Replayed {
        events: Vec<ProjectionEnvelope>,
        current_high_water: u64,
        next_cursor: ProjectionCursor,
    },
    Empty {
        current_high_water: u64,
        next_cursor: ProjectionCursor,
    },
    Resync {
        reason: ProjectionResyncReason,
        descriptor: Option<ProjectionStreamDescriptor>,
        requested_cursor: Option<ProjectionCursor>,
        snapshot: Option<ProjectionSnapshotBundle>,
    },
}

#[derive(Debug, Clone)]
pub enum AckResult {
    Accepted { last_acked_seq: u64, lag_count: u64 },
    Rejected { reason: String },
}

pub struct ProjectionReplayService {
    store: Arc<ProjectionReplayStore>,
    subscriptions: Arc<SubscriptionRegistry>,
    #[allow(dead_code)]
    retention_policy: RetentionPolicy,
    metrics: Arc<ProjectionReplayMetrics>,
    /// Pending receivers keyed by subscription ID. The transport layer
    /// takes the receiver once via `take_subscription_receiver` after
    /// subscribe returns; only one caller may take it.
    pending_receivers: Mutex<HashMap<String, tokio::sync::mpsc::Receiver<ProjectionEnvelope>>>,
}

impl ProjectionReplayService {
    pub fn new(store: Arc<ProjectionReplayStore>) -> Self {
        Self {
            store,
            subscriptions: Arc::new(SubscriptionRegistry::new(SubscriptionConfig::default())),
            retention_policy: RetentionPolicy::default(),
            metrics: Arc::new(ProjectionReplayMetrics::new()),
            pending_receivers: Mutex::new(HashMap::new()),
        }
    }

    pub fn with_config(
        store: Arc<ProjectionReplayStore>,
        subscription_config: SubscriptionConfig,
        retention_policy: RetentionPolicy,
    ) -> Self {
        Self {
            store,
            subscriptions: Arc::new(SubscriptionRegistry::new(subscription_config)),
            retention_policy,
            metrics: Arc::new(ProjectionReplayMetrics::new()),
            pending_receivers: Mutex::new(HashMap::new()),
        }
    }

    pub fn store(&self) -> &Arc<ProjectionReplayStore> {
        &self.store
    }

    pub fn subscriptions(&self) -> &Arc<SubscriptionRegistry> {
        &self.subscriptions
    }

    pub fn metrics(&self) -> &Arc<ProjectionReplayMetrics> {
        &self.metrics
    }

    pub fn metrics_snapshot(
        &self,
    ) -> crate::projection_replay::metrics::ProjectionReplayMetricsSnapshot {
        self.metrics.snapshot()
    }

    pub async fn publish_from_core(
        &self,
        source_envelope: &EventEnvelope<CoreEvent>,
    ) -> Result<PublishOutcome, StorageError> {
        self.publish_from_core_with_context(
            source_envelope,
            &ProjectionPublicationContext::default(),
        )
        .await
    }

    pub async fn publish_from_core_with_context(
        &self,
        source_envelope: &EventEnvelope<CoreEvent>,
        context: &ProjectionPublicationContext,
    ) -> Result<PublishOutcome, StorageError> {
        // Reject unbound session
        let session_id = match context.session_id.as_deref() {
            Some(s) if !s.is_empty() => s.to_string(),
            _ => {
                // Fall back to envelope session_id for backward compat
                let env_sid = source_envelope
                    .session_id
                    .as_deref()
                    .unwrap_or_default()
                    .to_string();
                if env_sid.is_empty() {
                    return Ok(PublishOutcome::Skipped {
                        reason: SafePublicationReason::UnboundSession,
                    });
                }
                env_sid
            }
        };

        let project_id = match context.project_id.as_deref() {
            Some(p) if !p.is_empty() => p.to_string(),
            _ => {
                return Ok(PublishOutcome::Skipped {
                    reason: SafePublicationReason::UnboundSession,
                });
            }
        };
        let workspace_id = context.workspace_id.clone();

        let projections = projection_events_from_core(source_envelope);
        if projections.is_empty() {
            return Ok(PublishOutcome::Skipped {
                reason: SafePublicationReason::AdaptionEmpty,
            });
        }

        // Resolve streams BEFORE opening the transaction to avoid
        // connection pool deadlock (stream creation uses pool directly).
        let mut session_stream_id: Option<String> = None;
        let mut project_stream_id: Option<String> = None;

        for (stream_kind, _) in &projections {
            match stream_kind {
                ProjectionStreamKind::Session => {
                    if session_stream_id.is_none() {
                        let (desc, _created) = self
                            .store
                            .get_or_create_session_stream_with_revision(
                                &session_id,
                                &project_id,
                                workspace_id.as_deref(),
                                context.binding_revision.max(1),
                            )
                            .await?;
                        session_stream_id = Some(desc.stream_id.0.clone());
                    }
                }
                ProjectionStreamKind::Project => {
                    if project_stream_id.is_none() {
                        let (desc, _created) =
                            self.store.get_or_create_project_stream(&project_id).await?;
                        project_stream_id = Some(desc.stream_id.0.clone());
                    }
                }
            }
        }

        // Now open a transaction for seq allocation + event insert + high water
        let mut session_seq = 0u64;
        let mut project_seq = 0u64;

        let mut tx = self.store.begin_tx().await?;

        for (stream_kind, proj_envelope) in &projections {
            let stream_id_str = match stream_kind {
                ProjectionStreamKind::Session => session_stream_id.as_deref(),
                ProjectionStreamKind::Project => project_stream_id.as_deref(),
            };
            let sid = match stream_id_str {
                Some(s) => s,
                None => continue,
            };

            let seq = self.store.next_event_seq_tx(&mut tx, sid).await?;
            self.store
                .insert_event_tx(&mut tx, sid, seq, proj_envelope)
                .await?;
            self.store.update_high_water_tx(&mut tx, sid, seq).await?;

            match stream_kind {
                ProjectionStreamKind::Session => session_seq = seq,
                ProjectionStreamKind::Project => project_seq = seq,
            }

            self.metrics
                .events_persisted_total
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        }

        sqlx::query("COMMIT")
            .execute(&mut *tx)
            .await
            .map_err(|e| StorageError::Database(e.to_string()))?;

        // Live delivery uses the ACTUAL persisted stream IDs, not synthetic ones
        for (stream_kind, proj_envelope) in &projections {
            let stream_id_str = match stream_kind {
                ProjectionStreamKind::Session => session_stream_id.as_deref(),
                ProjectionStreamKind::Project => project_stream_id.as_deref(),
            };
            if let Some(sid) = stream_id_str {
                let _ = self
                    .subscriptions
                    .deliver_to_stream(sid, proj_envelope.clone());
            }
        }

        Ok(PublishOutcome::Published {
            session_stream_seq: session_seq,
            project_stream_seq: project_seq,
        })
    }

    pub async fn subscribe_session(
        &self,
        session_id: &str,
        project_id: &str,
        workspace_id: Option<&str>,
        client_id: &str,
        request: &ProjectionSubscriptionRequest,
    ) -> Result<ProjectionSubscriptionId, StorageError> {
        let (desc, _created) = self
            .store
            .get_or_create_session_stream(session_id, project_id, workspace_id)
            .await?;

        let (sub_id, receiver) = self
            .subscriptions
            .register(
                client_id,
                &desc.stream_id,
                ProjectionStreamKind::Session,
                request.projection_version,
            )
            .map_err(|e| StorageError::Database(e.to_string()))?;

        self.subscriptions
            .set_live(&sub_id)
            .map_err(|e| StorageError::Database(e.to_string()))?;

        // Store the receiver so the transport layer can take it later
        self.pending_receivers
            .lock()
            .await
            .insert(sub_id.0.clone(), receiver);

        self.metrics
            .active_subscriptions
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);

        Ok(sub_id)
    }

    pub async fn subscribe_project(
        &self,
        project_id: &str,
        client_id: &str,
        request: &ProjectionSubscriptionRequest,
    ) -> Result<ProjectionSubscriptionId, StorageError> {
        let (desc, _created) = self.store.get_or_create_project_stream(project_id).await?;

        let (sub_id, receiver) = self
            .subscriptions
            .register(
                client_id,
                &desc.stream_id,
                ProjectionStreamKind::Project,
                request.projection_version,
            )
            .map_err(|e| StorageError::Database(e.to_string()))?;

        self.subscriptions
            .set_live(&sub_id)
            .map_err(|e| StorageError::Database(e.to_string()))?;

        // Store the receiver so the transport layer can take it later
        self.pending_receivers
            .lock()
            .await
            .insert(sub_id.0.clone(), receiver);

        self.metrics
            .active_subscriptions
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);

        Ok(sub_id)
    }

    /// Take the pending receiver for a subscription. Returns `None` if the
    /// receiver was already taken or the subscription does not exist.
    pub async fn take_subscription_receiver(
        &self,
        sub_id: &ProjectionSubscriptionId,
    ) -> Option<tokio::sync::mpsc::Receiver<ProjectionEnvelope>> {
        self.pending_receivers.lock().await.remove(&sub_id.0)
    }

    pub async fn resume(
        &self,
        subscription_id: &ProjectionSubscriptionId,
        cursor: &ProjectionCursor,
        _include_snapshot: bool,
    ) -> Result<ResumeOutcome, StorageError> {
        let sub = self
            .subscriptions
            .by_id()
            .get(subscription_id)
            .ok_or_else(|| StorageError::Database("subscription not found".into()))?
            .clone();

        let desc = match self
            .store
            .lookup_stream_by_id(cursor.stream_id.as_str())
            .await?
        {
            Some(d) => d,
            None => {
                self.metrics.increment_resync_reason("stream_mismatch");
                return Ok(ResumeOutcome::Resync {
                    reason: ProjectionResyncReason::StreamMismatch,
                    descriptor: None,
                    requested_cursor: Some(cursor.clone()),
                    snapshot: None,
                });
            }
        };

        if desc.high_water_seq == cursor.event_seq {
            return Ok(ResumeOutcome::Empty {
                current_high_water: desc.high_water_seq,
                next_cursor: ProjectionCursor {
                    stream_id: cursor.stream_id.clone(),
                    event_seq: desc.high_water_seq,
                    projection_version: desc.projection_version,
                },
            });
        }

        if cursor.event_seq > desc.high_water_seq {
            self.metrics.increment_resync_reason("cursor_ahead");
            return Ok(ResumeOutcome::Resync {
                reason: ProjectionResyncReason::CursorAhead,
                descriptor: Some(desc),
                requested_cursor: Some(cursor.clone()),
                snapshot: None,
            });
        }

        if cursor.event_seq < desc.retention_floor_seq {
            self.metrics.increment_resync_reason("history_expired");
            return Ok(ResumeOutcome::Resync {
                reason: ProjectionResyncReason::HistoryExpired,
                descriptor: Some(desc),
                requested_cursor: Some(cursor.clone()),
                snapshot: None,
            });
        }

        if sub.projection_version != desc.projection_version {
            self.metrics.increment_resync_reason("version_mismatch");
            return Ok(ResumeOutcome::Resync {
                reason: ProjectionResyncReason::VersionMismatch,
                descriptor: Some(desc),
                requested_cursor: Some(cursor.clone()),
                snapshot: None,
            });
        }

        let events = self
            .store
            .events_after(
                cursor.stream_id.as_str(),
                cursor.event_seq,
                MAX_REPLAY_EVENTS,
                MAX_REPLAY_BYTES,
            )
            .await?;

        let last_seq = events
            .last()
            .map(|e| e.event_seq)
            .unwrap_or(cursor.event_seq);
        let next_cursor = ProjectionCursor {
            stream_id: cursor.stream_id.clone(),
            event_seq: last_seq,
            projection_version: desc.projection_version,
        };

        let envelopes: Vec<ProjectionEnvelope> = events
            .iter()
            .filter_map(|row| serde_json::from_str(&row.payload_json).ok())
            .collect();

        if envelopes.is_empty() && cursor.event_seq < desc.high_water_seq {
            self.metrics.increment_resync_reason("history_gap");
            return Ok(ResumeOutcome::Resync {
                reason: ProjectionResyncReason::HistoryGap,
                descriptor: Some(desc),
                requested_cursor: Some(cursor.clone()),
                snapshot: None,
            });
        }

        Ok(ResumeOutcome::Replayed {
            events: envelopes,
            current_high_water: desc.high_water_seq,
            next_cursor,
        })
    }

    pub async fn ack(
        &self,
        subscription_id: &ProjectionSubscriptionId,
        cursor: &ProjectionCursor,
    ) -> Result<AckResult, StorageError> {
        let desc = self
            .store
            .lookup_stream_by_id(cursor.stream_id.as_str())
            .await?
            .ok_or_else(|| StorageError::Database("stream not found".into()))?;

        match self.subscriptions.ack(
            subscription_id,
            cursor.event_seq,
            &cursor.stream_id,
            cursor.projection_version,
            desc.high_water_seq,
        ) {
            Ok(lag) => Ok(AckResult::Accepted {
                last_acked_seq: cursor.event_seq,
                lag_count: lag,
            }),
            Err(e) => Ok(AckResult::Rejected {
                reason: e.to_string(),
            }),
        }
    }

    pub async fn unsubscribe(
        &self,
        subscription_id: &ProjectionSubscriptionId,
    ) -> Result<(), StorageError> {
        self.subscriptions
            .unsubscribe(subscription_id)
            .map_err(|e| StorageError::Database(e.to_string()))
    }

    pub async fn maintenance_tick(
        &self,
        now_ms: i64,
    ) -> Result<crate::projection_replay::retention::MaintenanceReport, StorageError> {
        self.retention_policy
            .maintenance_tick(&self.store, now_ms)
            .await
    }
}

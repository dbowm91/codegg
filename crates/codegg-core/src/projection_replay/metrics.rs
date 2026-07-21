use std::sync::atomic::{AtomicI64, AtomicU64, Ordering};

use dashmap::DashMap;
use serde::{Deserialize, Serialize};

use crate::projection_replay::policy::DisclosureReason;

#[derive(Debug, Default)]
pub struct ProjectionReplayMetrics {
    pub stream_count_total: AtomicU64,
    pub stream_count_session: AtomicU64,
    pub stream_count_project: AtomicU64,
    pub events_persisted_total: AtomicU64,
    pub events_persisted_bytes_total: AtomicU64,
    pub events_skipped_internal: AtomicU64,
    pub events_skipped_client_local: AtomicU64,
    pub events_skipped_sensitive_redacted: AtomicU64,
    pub publication_failures: AtomicU64,
    pub checkpoints_written_total: AtomicU64,
    pub checkpoint_latest_age_ms: AtomicI64,
    pub pruned_events_total: AtomicU64,
    pub pruned_bytes_total: AtomicU64,
    pub active_subscriptions: AtomicU64,
    pub subscriber_queue_depth: AtomicU64,
    pub subscriber_lag_total: AtomicU64,
    pub ack_rejections_total: AtomicU64,
    pub resync_count_by_reason: DashMap<String, u64>,
    pub corrupt_quarantined_streams: AtomicU64,
    pub denials_by_reason: DashMap<String, u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProjectionReplayMetricsSnapshot {
    pub stream_count_total: u64,
    pub stream_count_session: u64,
    pub stream_count_project: u64,
    pub events_persisted_total: u64,
    pub events_persisted_bytes_total: u64,
    pub events_skipped_internal: u64,
    pub events_skipped_client_local: u64,
    pub events_skipped_sensitive_redacted: u64,
    pub publication_failures: u64,
    pub checkpoints_written_total: u64,
    pub checkpoint_latest_age_ms: i64,
    pub pruned_events_total: u64,
    pub pruned_bytes_total: u64,
    pub active_subscriptions: u64,
    pub subscriber_queue_depth: u64,
    pub subscriber_lag_total: u64,
    pub ack_rejections_total: u64,
    pub resync_count_by_reason: std::collections::HashMap<String, u64>,
    pub corrupt_quarantined_streams: u64,
    pub denials_by_reason: std::collections::HashMap<String, u64>,
}

impl ProjectionReplayMetrics {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn increment_resync_reason(&self, reason: &str) {
        *self
            .resync_count_by_reason
            .entry(reason.to_string())
            .or_insert(0) += 1;
    }

    pub fn increment_denials_by_reason(&self, reason: DisclosureReason) {
        *self
            .denials_by_reason
            .entry(reason.as_str().to_string())
            .or_insert(0) += 1;
    }

    pub fn snapshot(&self) -> ProjectionReplayMetricsSnapshot {
        ProjectionReplayMetricsSnapshot {
            stream_count_total: self.stream_count_total.load(Ordering::Relaxed),
            stream_count_session: self.stream_count_session.load(Ordering::Relaxed),
            stream_count_project: self.stream_count_project.load(Ordering::Relaxed),
            events_persisted_total: self.events_persisted_total.load(Ordering::Relaxed),
            events_persisted_bytes_total: self.events_persisted_bytes_total.load(Ordering::Relaxed),
            events_skipped_internal: self.events_skipped_internal.load(Ordering::Relaxed),
            events_skipped_client_local: self.events_skipped_client_local.load(Ordering::Relaxed),
            events_skipped_sensitive_redacted: self
                .events_skipped_sensitive_redacted
                .load(Ordering::Relaxed),
            publication_failures: self.publication_failures.load(Ordering::Relaxed),
            checkpoints_written_total: self.checkpoints_written_total.load(Ordering::Relaxed),
            checkpoint_latest_age_ms: self.checkpoint_latest_age_ms.load(Ordering::Relaxed),
            pruned_events_total: self.pruned_events_total.load(Ordering::Relaxed),
            pruned_bytes_total: self.pruned_bytes_total.load(Ordering::Relaxed),
            active_subscriptions: self.active_subscriptions.load(Ordering::Relaxed),
            subscriber_queue_depth: self.subscriber_queue_depth.load(Ordering::Relaxed),
            subscriber_lag_total: self.subscriber_lag_total.load(Ordering::Relaxed),
            ack_rejections_total: self.ack_rejections_total.load(Ordering::Relaxed),
            resync_count_by_reason: self
                .resync_count_by_reason
                .iter()
                .map(|e| (e.key().clone(), *e.value()))
                .collect(),
            corrupt_quarantined_streams: self.corrupt_quarantined_streams.load(Ordering::Relaxed),
            denials_by_reason: self
                .denials_by_reason
                .iter()
                .map(|e| (e.key().clone(), *e.value()))
                .collect(),
        }
    }
}

use crate::error::StorageError;

use super::store::ProjectionReplayStore;

#[derive(Debug, Clone)]
pub struct RetentionPolicy {
    pub session_max_events: usize,
    pub session_max_age_ms: i64,
    pub session_max_bytes: u64,
    pub project_max_events: usize,
    pub project_max_age_ms: i64,
    pub project_max_bytes: u64,
    pub hard_event_byte_cap: usize,
    pub checkpoint_event_interval: usize,
    pub checkpoint_byte_interval: u64,
    pub checkpoint_min_interval_ms: i64,
    pub max_checkpoints_per_stream: usize,
}

impl Default for RetentionPolicy {
    fn default() -> Self {
        Self {
            session_max_events: 20_000,
            session_max_age_ms: 7 * 24 * 60 * 60 * 1000,
            session_max_bytes: 64 * 1024 * 1024,
            project_max_events: 50_000,
            project_max_age_ms: 7 * 24 * 60 * 60 * 1000,
            project_max_bytes: 128 * 1024 * 1024,
            hard_event_byte_cap: 64 * 1024,
            checkpoint_event_interval: 256,
            checkpoint_byte_interval: 1024 * 1024,
            checkpoint_min_interval_ms: 5 * 60 * 1000,
            max_checkpoints_per_stream: 4,
        }
    }
}

#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
pub struct MaintenanceReport {
    pub streams_checked: usize,
    pub checkpoints_written: usize,
    pub events_pruned: usize,
    pub bytes_pruned: u64,
    pub streams_quarantined: usize,
}

impl RetentionPolicy {
    pub async fn maintenance_tick(
        &self,
        store: &ProjectionReplayStore,
        _now_ms: i64,
    ) -> Result<MaintenanceReport, StorageError> {
        let mut report = MaintenanceReport::default();
        let streams = store.load_active_streams().await?;

        for desc in &streams {
            report.streams_checked += 1;

            let high_water = desc.high_water_seq;
            let retention_floor = desc.retention_floor_seq;
            let events = store
                .events_after(
                    desc.stream_id.as_str(),
                    retention_floor,
                    usize::MAX,
                    u64::MAX,
                )
                .await?;
            let event_count = events.len();
            let _event_bytes: u64 = events.iter().map(|e| e.payload_bytes as u64).sum();

            let max_events = match desc.kind {
                codegg_protocol::projection::replay::ProjectionStreamKind::Session => {
                    self.session_max_events
                }
                codegg_protocol::projection::replay::ProjectionStreamKind::Project => {
                    self.project_max_events
                }
            };

            if event_count > max_events {
                let excess = event_count - max_events;
                let new_floor = high_water
                    .saturating_sub(max_events as u64)
                    .saturating_add(1);
                let _ = new_floor.max(retention_floor + excess as u64);
                let pruned = store
                    .prune_before(desc.stream_id.as_str(), new_floor)
                    .await?;
                report.events_pruned += pruned;
            }

            if high_water > 0 {
                let events_since_checkpoint = if let Some(cp) = desc.latest_checkpoint_seq {
                    (high_water - cp) as usize
                } else {
                    high_water as usize
                };

                if events_since_checkpoint >= self.checkpoint_event_interval {
                    report.checkpoints_written += 1;
                }
            }

            store
                .prune_old_checkpoints(desc.stream_id.as_str(), self.max_checkpoints_per_stream)
                .await?;
        }

        Ok(report)
    }
}

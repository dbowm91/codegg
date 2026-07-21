use std::collections::HashSet;
use std::sync::atomic::{AtomicU64, Ordering};

use chrono::Utc;
use dashmap::DashMap;
use tokio::sync::mpsc;

use codegg_protocol::projection::event::ProjectionEnvelope;
use codegg_protocol::projection::replay::{
    ProjectionStreamId, ProjectionStreamKind, ProjectionSubscriptionId,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SubscriptionState {
    Initializing,
    Live,
    ResyncRequired,
    Closed,
}

#[derive(Debug, Clone)]
pub struct SubscriptionEntry {
    pub id: ProjectionSubscriptionId,
    pub stream_id: ProjectionStreamId,
    pub scope: ProjectionStreamKind,
    pub client_id: String,
    pub projection_version: u32,
    pub last_delivered_seq: u64,
    pub last_acked_seq: u64,
    pub sender: mpsc::Sender<ProjectionEnvelope>,
    pub state: SubscriptionState,
    pub created_at_ms: i64,
    pub last_activity_ms: i64,
}

#[derive(Debug, Clone)]
pub struct SubscriptionConfig {
    pub max_per_client: usize,
    pub max_per_daemon: usize,
    pub queue_capacity: usize,
    pub idle_timeout_ms: i64,
}

impl Default for SubscriptionConfig {
    fn default() -> Self {
        Self {
            max_per_client: 32,
            max_per_daemon: 256,
            queue_capacity: 512,
            idle_timeout_ms: 30 * 60 * 1000,
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum SubscriptionError {
    #[error("subscription limit exceeded for client")]
    PerClientLimit,
    #[error("subscription limit exceeded globally")]
    GlobalLimit,
    #[error("subscription not found")]
    NotFound,
    #[error("stream mismatch")]
    StreamMismatch,
    #[error("version mismatch")]
    VersionMismatch,
    #[error("cursor ahead of high water")]
    CursorAhead,
    #[error("subscription is lagged and requires resync")]
    ResyncRequired,
}

pub struct SubscriptionRegistry {
    by_id: DashMap<ProjectionSubscriptionId, SubscriptionEntry>,
    by_client: DashMap<String, HashSet<String>>,
    by_stream: DashMap<String, HashSet<String>>,
    config: SubscriptionConfig,
    active_count: AtomicU64,
}

impl SubscriptionRegistry {
    pub fn new(config: SubscriptionConfig) -> Self {
        Self {
            by_id: DashMap::new(),
            by_client: DashMap::new(),
            by_stream: DashMap::new(),
            config,
            active_count: AtomicU64::new(0),
        }
    }

    pub fn active_count(&self) -> u64 {
        self.active_count.load(Ordering::Relaxed)
    }

    pub fn register(
        &self,
        client_id: &str,
        stream_id: &ProjectionStreamId,
        scope: ProjectionStreamKind,
        projection_version: u32,
    ) -> Result<(ProjectionSubscriptionId, mpsc::Receiver<ProjectionEnvelope>), SubscriptionError>
    {
        let mut client_subs = self.by_client.entry(client_id.to_string()).or_default();
        if client_subs.len() >= self.config.max_per_client {
            return Err(SubscriptionError::PerClientLimit);
        }
        if self.active_count.load(Ordering::Relaxed) >= self.config.max_per_daemon as u64 {
            return Err(SubscriptionError::GlobalLimit);
        }

        let sub_id = ProjectionSubscriptionId(uuid::Uuid::new_v4().to_string());
        let now = Utc::now().timestamp_millis();
        let (sender, receiver) = mpsc::channel(self.config.queue_capacity);

        let entry = SubscriptionEntry {
            id: sub_id.clone(),
            stream_id: stream_id.clone(),
            scope,
            client_id: client_id.to_string(),
            projection_version,
            last_delivered_seq: 0,
            last_acked_seq: 0,
            sender,
            state: SubscriptionState::Initializing,
            created_at_ms: now,
            last_activity_ms: now,
        };

        self.by_id.insert(sub_id.clone(), entry);
        client_subs.insert(sub_id.0.clone());
        self.by_stream
            .entry(stream_id.0.clone())
            .or_default()
            .insert(sub_id.0.clone());
        self.active_count.fetch_add(1, Ordering::Relaxed);

        Ok((sub_id, receiver))
    }

    pub fn set_live(&self, id: &ProjectionSubscriptionId) -> Result<(), SubscriptionError> {
        let mut entry = self.by_id.get_mut(id).ok_or(SubscriptionError::NotFound)?;
        entry.state = SubscriptionState::Live;
        Ok(())
    }

    pub fn set_resync_required(
        &self,
        id: &ProjectionSubscriptionId,
    ) -> Result<(), SubscriptionError> {
        let mut entry = self.by_id.get_mut(id).ok_or(SubscriptionError::NotFound)?;
        entry.state = SubscriptionState::ResyncRequired;
        Ok(())
    }

    pub fn ack(
        &self,
        id: &ProjectionSubscriptionId,
        event_seq: u64,
        stream_id: &ProjectionStreamId,
        projection_version: u32,
        high_water_seq: u64,
    ) -> Result<u64, SubscriptionError> {
        let mut entry = self.by_id.get_mut(id).ok_or(SubscriptionError::NotFound)?;

        if entry.stream_id != *stream_id {
            return Err(SubscriptionError::StreamMismatch);
        }
        if entry.projection_version != projection_version {
            return Err(SubscriptionError::VersionMismatch);
        }
        if event_seq > high_water_seq {
            return Err(SubscriptionError::CursorAhead);
        }

        if event_seq <= entry.last_acked_seq {
            return Ok(entry.last_acked_seq);
        }

        entry.last_acked_seq = event_seq;
        entry.last_activity_ms = Utc::now().timestamp_millis();
        let lag = high_water_seq.saturating_sub(event_seq);
        Ok(lag)
    }

    pub fn deliver_to_stream(
        &self,
        stream_id: &str,
        envelope: ProjectionEnvelope,
    ) -> Result<usize, SubscriptionError> {
        let mut delivered = 0;
        if let Some(subs) = self.by_stream.get(stream_id) {
            let sub_ids: Vec<String> = subs.iter().cloned().collect();
            for sub_id_str in sub_ids {
                if let Some(entry) = self
                    .by_id
                    .get(&ProjectionSubscriptionId(sub_id_str.clone()))
                {
                    if entry.state == SubscriptionState::Live {
                        match entry.sender.try_send(envelope.clone()) {
                            Ok(()) => delivered += 1,
                            Err(mpsc::error::TrySendError::Full(_)) => {
                                drop(entry);
                                if let Some(mut e) =
                                    self.by_id.get_mut(&ProjectionSubscriptionId(sub_id_str))
                                {
                                    e.state = SubscriptionState::ResyncRequired;
                                }
                            }
                            Err(mpsc::error::TrySendError::Closed(_)) => {}
                        }
                    }
                }
            }
        }
        Ok(delivered)
    }

    pub fn unsubscribe(&self, id: &ProjectionSubscriptionId) -> Result<(), SubscriptionError> {
        let entry = self.by_id.remove(id).ok_or(SubscriptionError::NotFound)?.1;

        if let Some(mut client_subs) = self.by_client.get_mut(&entry.client_id) {
            client_subs.remove(&entry.id.0);
        }
        if let Some(mut stream_subs) = self.by_stream.get_mut(&entry.stream_id.0) {
            stream_subs.remove(&entry.id.0);
        }
        self.active_count.fetch_sub(1, Ordering::Relaxed);
        Ok(())
    }

    pub fn gc_idle(&self, now_ms: i64) -> usize {
        let mut closed = 0;
        let ids: Vec<ProjectionSubscriptionId> = self
            .by_id
            .iter()
            .filter(|e| {
                e.value().state != SubscriptionState::Closed
                    && (now_ms - e.value().last_activity_ms) > self.config.idle_timeout_ms
            })
            .map(|e| e.key().clone())
            .collect();

        for id in ids {
            if let Some(mut entry) = self.by_id.get_mut(&id) {
                entry.state = SubscriptionState::Closed;
            }
            if let Ok(()) = self.unsubscribe(&id) {
                closed += 1;
            }
        }
        closed
    }

    pub fn by_id(&self) -> &DashMap<ProjectionSubscriptionId, SubscriptionEntry> {
        &self.by_id
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_registry() -> SubscriptionRegistry {
        SubscriptionRegistry::new(SubscriptionConfig {
            max_per_client: 2,
            max_per_daemon: 4,
            queue_capacity: 8,
            idle_timeout_ms: 60_000,
        })
    }

    #[test]
    fn register_and_subscribe() {
        let reg = test_registry();
        let sid = ProjectionStreamId::new("s1").unwrap();
        let (sub_id, _rx) = reg
            .register("client1", &sid, ProjectionStreamKind::Session, 1)
            .unwrap();
        assert_eq!(reg.active_count(), 1);
        reg.set_live(&sub_id).unwrap();
        reg.unsubscribe(&sub_id).unwrap();
        assert_eq!(reg.active_count(), 0);
    }

    #[test]
    fn per_client_limit_enforced() {
        let reg = test_registry();
        let sid = ProjectionStreamId::new("s1").unwrap();
        let _ = reg
            .register("c1", &sid, ProjectionStreamKind::Session, 1)
            .unwrap();
        let _ = reg
            .register("c1", &sid, ProjectionStreamKind::Session, 1)
            .unwrap();
        assert!(matches!(
            reg.register("c1", &sid, ProjectionStreamKind::Session, 1),
            Err(SubscriptionError::PerClientLimit)
        ));
    }

    #[test]
    fn global_limit_enforced() {
        let reg = test_registry();
        let sid = ProjectionStreamId::new("s1").unwrap();
        for i in 0..4 {
            let _ = reg
                .register(&format!("c{i}"), &sid, ProjectionStreamKind::Session, 1)
                .unwrap();
        }
        assert!(matches!(
            reg.register("c5", &sid, ProjectionStreamKind::Session, 1),
            Err(SubscriptionError::GlobalLimit)
        ));
    }

    #[test]
    fn ack_monotonicity() {
        let reg = test_registry();
        let sid = ProjectionStreamId::new("s1").unwrap();
        let (sub_id, _rx) = reg
            .register("c1", &sid, ProjectionStreamKind::Session, 1)
            .unwrap();

        let lag = reg.ack(&sub_id, 5, &sid, 1, 10).unwrap();
        assert_eq!(lag, 5);

        let lag = reg.ack(&sub_id, 3, &sid, 1, 10).unwrap();
        assert_eq!(lag, 5);

        let lag = reg.ack(&sub_id, 10, &sid, 1, 10).unwrap();
        assert_eq!(lag, 0);
    }

    #[test]
    fn ack_stream_mismatch() {
        let reg = test_registry();
        let sid = ProjectionStreamId::new("s1").unwrap();
        let sid2 = ProjectionStreamId::new("s2").unwrap();
        let (sub_id, _rx) = reg
            .register("c1", &sid, ProjectionStreamKind::Session, 1)
            .unwrap();
        assert!(matches!(
            reg.ack(&sub_id, 1, &sid2, 1, 10),
            Err(SubscriptionError::StreamMismatch)
        ));
    }

    #[test]
    fn unsubscribe_removes_subscription() {
        let reg = test_registry();
        let sid = ProjectionStreamId::new("s1").unwrap();
        let (sub_id, _rx) = reg
            .register("c1", &sid, ProjectionStreamKind::Session, 1)
            .unwrap();
        reg.set_live(&sub_id).unwrap();
        reg.unsubscribe(&sub_id).unwrap();
        assert_eq!(reg.active_count(), 0);
    }
}

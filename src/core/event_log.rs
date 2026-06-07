use std::collections::VecDeque;
use std::sync::atomic::{AtomicU64, Ordering};
use tokio::sync::{broadcast, Mutex};

use crate::protocol::core::{CoreEvent, EventEnvelope, PROTOCOL_VERSION};

/// Events that are persisted to SQLite for recovery after restart.
/// High-value events: turn lifecycle, tool lifecycle, permissions, questions,
/// subagent lifecycle, errors. Deltas and snapshots are excluded.
fn should_persist(event: &CoreEvent) -> bool {
    matches!(
        event,
        CoreEvent::TurnStarted { .. }
            | CoreEvent::TurnCompleted { .. }
            | CoreEvent::TurnFailed { .. }
            | CoreEvent::ToolStarted { .. }
            | CoreEvent::ToolCompleted { .. }
            | CoreEvent::PermissionPending { .. }
            | CoreEvent::QuestionPending { .. }
            | CoreEvent::SubagentStarted { .. }
            | CoreEvent::SubagentCompleted { .. }
            | CoreEvent::SubagentFailed { .. }
            | CoreEvent::SessionUpdated { .. }
            | CoreEvent::Error { .. }
    )
}

pub struct EventLog {
    next_seq: AtomicU64,
    ring: Mutex<VecDeque<EventEnvelope<CoreEvent>>>,
    tx: broadcast::Sender<EventEnvelope<CoreEvent>>,
    capacity: usize,
    pool: Option<sqlx::SqlitePool>,
}

#[derive(Debug, Clone, Default)]
pub struct EventFilter {
    pub session_id: Option<String>,
    pub client_id: Option<String>,
    pub include_global: bool,
}

impl EventLog {
    pub fn new(capacity: usize) -> Self {
        let (tx, _) = broadcast::channel(capacity);
        Self {
            next_seq: AtomicU64::new(1),
            ring: Mutex::new(VecDeque::with_capacity(capacity)),
            tx,
            capacity,
            pool: None,
        }
    }

    pub fn new_with_pool(capacity: usize, pool: sqlx::SqlitePool) -> Self {
        let (tx, _) = broadcast::channel(capacity);
        Self {
            next_seq: AtomicU64::new(1),
            ring: Mutex::new(VecDeque::with_capacity(capacity)),
            tx,
            capacity,
            pool: Some(pool),
        }
    }

    /// Publish an event. Returns the assigned sequence number.
    pub async fn publish(
        &self,
        session_id: Option<String>,
        turn_id: Option<String>,
        payload: CoreEvent,
    ) -> u64 {
        let seq = self.next_seq.fetch_add(1, Ordering::SeqCst);
        let envelope = EventEnvelope {
            protocol_version: PROTOCOL_VERSION,
            event_seq: seq,
            timestamp_ms: chrono::Utc::now().timestamp_millis(),
            session_id: session_id.clone(),
            turn_id: turn_id.clone(),
            payload,
        };

        {
            let mut ring = self.ring.lock().await;
            if ring.len() >= self.capacity {
                ring.pop_front();
            }
            ring.push_back(envelope.clone());
        }

        // Persist important events to SQLite (best-effort)
        if let Some(ref pool) = self.pool {
            if should_persist(&envelope.payload) {
                let event_type = format!("{:?}", std::mem::discriminant(&envelope.payload));
                if let Ok(payload_json) = serde_json::to_string(&envelope.payload) {
                    let _ = sqlx::query(
                        "INSERT OR IGNORE INTO core_event_log \
                         (event_seq, session_id, turn_id, event_type, payload_json) \
                         VALUES (?, ?, ?, ?, ?)",
                    )
                    .bind(seq as i64)
                    .bind(&session_id)
                    .bind(&turn_id)
                    .bind(&event_type)
                    .bind(&payload_json)
                    .execute(pool)
                    .await;
                }
            }
        }

        let _ = self.tx.send(envelope);

        seq
    }

    /// Subscribe to live events.
    pub fn subscribe(&self) -> broadcast::Receiver<EventEnvelope<CoreEvent>> {
        self.tx.subscribe()
    }

    /// Replay events from a given sequence number, filtered.
    /// Falls through to SQLite when the ring buffer doesn't cover the requested sequence.
    pub async fn replay_from(
        &self,
        from_event_seq: u64,
        filter: &EventFilter,
    ) -> Vec<EventEnvelope<CoreEvent>> {
        let ring_events = {
            let ring = self.ring.lock().await;
            ring.iter()
                .filter(|e| e.event_seq >= from_event_seq)
                .filter(|e| {
                    if let Some(ref sid) = filter.session_id {
                        e.session_id.as_deref() == Some(sid.as_str())
                    } else if filter.include_global {
                        true
                    } else {
                        e.session_id.is_none()
                    }
                })
                .cloned()
                .collect::<Vec<_>>()
        };

        // If ring buffer has events from the requested seq, use them
        if !ring_events.is_empty() {
            let earliest_seq = ring_events.first().map(|e| e.event_seq).unwrap_or(0);
            if earliest_seq <= from_event_seq {
                return ring_events;
            }
        }

        // Ring buffer doesn't cover it - fall through to DB
        if self.pool.is_some() {
            self.replay_from_db(from_event_seq, filter).await
        } else {
            ring_events
        }
    }

    /// Replay events from SQLite when the ring buffer doesn't have them.
    /// Returns events ordered by event_seq ASC.
    pub async fn replay_from_db(
        &self,
        from_event_seq: u64,
        filter: &EventFilter,
    ) -> Vec<EventEnvelope<CoreEvent>> {
        let Some(ref pool) = self.pool else {
            return Vec::new();
        };

        let mut query = String::from(
            "SELECT event_seq, session_id, turn_id, event_type, payload_json, created_at \
             FROM core_event_log WHERE event_seq >= ?",
        );
        let bind_seq: i64 = from_event_seq as i64;

        // We can't dynamically add bind parameters with sqlx query builder easily,
        // so we handle session_id filtering separately.
        let rows: Vec<CoreEventRow> = if let Some(ref sid) = filter.session_id {
            query.push_str(" AND session_id = ? ORDER BY event_seq ASC");
            sqlx::query_as::<_, CoreEventRow>(&query)
                .bind(bind_seq)
                .bind(sid)
                .fetch_all(pool)
                .await
                .unwrap_or_default()
        } else if filter.include_global {
            query.push_str(" ORDER BY event_seq ASC");
            sqlx::query_as::<_, CoreEventRow>(&query)
                .bind(bind_seq)
                .fetch_all(pool)
                .await
                .unwrap_or_default()
        } else {
            query.push_str(" AND session_id IS NULL ORDER BY event_seq ASC");
            sqlx::query_as::<_, CoreEventRow>(&query)
                .bind(bind_seq)
                .fetch_all(pool)
                .await
                .unwrap_or_default()
        };

        rows.into_iter()
            .filter_map(|row| {
                let payload: CoreEvent = serde_json::from_str(&row.payload_json).ok()?;
                let timestamp_ms = chrono::DateTime::parse_from_rfc3339(&row.created_at)
                    .map(|dt| dt.timestamp_millis())
                    .unwrap_or(0);
                Some(EventEnvelope {
                    protocol_version: PROTOCOL_VERSION,
                    event_seq: row.event_seq as u64,
                    timestamp_ms,
                    session_id: row.session_id,
                    turn_id: row.turn_id,
                    payload,
                })
            })
            .collect()
    }

    /// Get the current latest sequence number.
    pub fn current_seq(&self) -> u64 {
        self.next_seq.load(Ordering::SeqCst)
    }

    /// Check if events exist from the requested sequence (ring buffer or DB).
    pub async fn has_events_from(&self, from_event_seq: u64) -> bool {
        // Check ring buffer first
        {
            let ring = self.ring.lock().await;
            if let Some(front) = ring.front() {
                if front.event_seq <= from_event_seq {
                    return true;
                }
            }
        }

        // Check DB
        if self.pool.is_some() {
            self.has_events_in_db(from_event_seq).await
        } else {
            false
        }
    }

    /// Check if the DB has events from the requested sequence.
    pub async fn has_events_in_db(&self, from_event_seq: u64) -> bool {
        let Some(ref pool) = self.pool else {
            return false;
        };
        sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM core_event_log WHERE event_seq >= ?")
            .bind(from_event_seq as i64)
            .fetch_one(pool)
            .await
            .map(|count| count > 0)
            .unwrap_or(false)
    }
}

#[derive(sqlx::FromRow)]
struct CoreEventRow {
    event_seq: i64,
    session_id: Option<String>,
    turn_id: Option<String>,
    #[allow(dead_code)]
    event_type: String,
    payload_json: String,
    #[allow(dead_code)]
    created_at: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn publish_assigns_sequential_ids() {
        let log = EventLog::new(100);
        let s1 = log
            .publish(
                None,
                None,
                CoreEvent::Error {
                    code: "e1".into(),
                    message: "m1".into(),
                },
            )
            .await;
        let s2 = log
            .publish(
                None,
                None,
                CoreEvent::Error {
                    code: "e2".into(),
                    message: "m2".into(),
                },
            )
            .await;
        assert_eq!(s1, 1);
        assert_eq!(s2, 2);
    }

    #[tokio::test]
    async fn ring_buffer_bounded() {
        let log = EventLog::new(3);
        for i in 0..5 {
            log.publish(
                None,
                None,
                CoreEvent::Error {
                    code: format!("e{}", i),
                    message: "m".into(),
                },
            )
            .await;
        }
        let events = log
            .replay_from(
                1,
                &EventFilter {
                    session_id: None,
                    client_id: None,
                    include_global: true,
                },
            )
            .await;
        assert_eq!(events.len(), 3);
        assert_eq!(events[0].event_seq, 3);
    }

    #[tokio::test]
    async fn replay_filters_by_session() {
        let log = EventLog::new(100);
        log.publish(
            Some("s1".into()),
            None,
            CoreEvent::Error {
                code: "e1".into(),
                message: "m".into(),
            },
        )
        .await;
        log.publish(
            Some("s2".into()),
            None,
            CoreEvent::Error {
                code: "e2".into(),
                message: "m".into(),
            },
        )
        .await;

        let filter = EventFilter {
            session_id: Some("s1".into()),
            client_id: None,
            include_global: false,
        };
        let events = log.replay_from(1, &filter).await;
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].session_id.as_deref(), Some("s1"));
    }

    #[tokio::test]
    async fn subscriber_receives_live_events() {
        let log = EventLog::new(100);
        let mut rx = log.subscribe();
        log.publish(
            None,
            None,
            CoreEvent::Error {
                code: "e1".into(),
                message: "m".into(),
            },
        )
        .await;
        let event = rx.recv().await.unwrap();
        assert_eq!(event.event_seq, 1);
    }

    #[test]
    fn should_persist_turn_events() {
        assert!(should_persist(&CoreEvent::TurnStarted {
            session_id: "s".into(),
            turn_id: "t".into(),
        }));
        assert!(should_persist(&CoreEvent::TurnCompleted {
            session_id: "s".into(),
            turn_id: "t".into(),
            stop_reason: "ok".into(),
        }));
        assert!(!should_persist(&CoreEvent::TurnTextDelta {
            session_id: "s".into(),
            turn_id: "t".into(),
            delta: "x".into(),
        }));
    }
}

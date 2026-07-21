use std::collections::VecDeque;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use tokio::sync::{broadcast, Mutex};

use crate::protocol::core::{CoreEvent, EventEnvelope, PROTOCOL_VERSION};

/// Trait for receiving projection events from the centralized EventLog sink.
///
/// Implementations must be `Send + Sync` and handle the envelope asynchronously.
/// The projection replay seam is the canonical production implementation.
pub trait ProjectionSink: Send + Sync {
    fn publish(
        &self,
        envelope: EventEnvelope<CoreEvent>,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = ()> + Send + 'static>>;
}

/// Apply a `filter` to a single envelope. See `EventLog::replay_from` and
/// `daemon_socket::event_matches_filter` for the semantic contract; this
/// helper is the single source of truth for in-memory matching and is
/// mirrored in `replay_from_db`.
fn filter_matches(filter: &EventFilter, event: &EventEnvelope<CoreEvent>) -> bool {
    match (&filter.session_id, filter.include_global) {
        (Some(sid), true) => {
            event.session_id.as_deref() == Some(sid.as_str()) || event.session_id.is_none()
        }
        (Some(sid), false) => event.session_id.as_deref() == Some(sid.as_str()),
        (None, _) => event.session_id.is_none(),
    }
}

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
            | CoreEvent::AssetRefreshCompleted { .. }
            | CoreEvent::Error { .. }
    )
}

pub struct EventLog {
    next_seq: AtomicU64,
    ring: Mutex<VecDeque<EventEnvelope<CoreEvent>>>,
    tx: broadcast::Sender<EventEnvelope<CoreEvent>>,
    capacity: usize,
    pool: Option<sqlx::SqlitePool>,
    projection_sink: Option<Arc<dyn ProjectionSink>>,
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
            projection_sink: None,
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
            projection_sink: None,
        }
    }

    /// Install a projection replay sink. The sink receives every envelope
    /// published through this log, exactly once per envelope.
    pub fn install_projection_sink(&mut self, sink: Arc<dyn ProjectionSink>) {
        self.projection_sink = Some(sink);
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
                let event_type = super::core_event_type(&envelope.payload).to_string();
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

        // Projection replay sink: invoked exactly once per published envelope.
        // The sink observes the canonical envelope and routes accepted events
        // into `projection_event` storage. `ProjectionStreamEvent` itself is
        // classified `Internal` by the safe-publication gate and never recurses.
        if let Some(ref sink) = self.projection_sink {
            let sink_env = envelope.clone();
            let sink = Arc::clone(sink);
            tokio::spawn(async move {
                sink.publish(sink_env).await;
            });
        }

        let _ = self.tx.send(envelope);

        seq
    }

    /// Subscribe to live events.
    pub fn subscribe(&self) -> broadcast::Receiver<EventEnvelope<CoreEvent>> {
        self.tx.subscribe()
    }

    /// Replay events strictly after `from_event_seq` (i.e. with
    /// `event_seq > from_event_seq`). `from_event_seq` is treated as
    /// the last sequence already seen by the client; pass `0` to
    /// replay from the very first event.
    /// Falls through to SQLite when the ring buffer doesn't cover the requested sequence.
    ///
    /// Filter semantics (must match `daemon_socket::event_matches_filter`):
    ///
    /// - `session_id: Some(sid), include_global: true`  -> events for `sid` plus
    ///   sessionless/global events.
    /// - `session_id: Some(sid), include_global: false` -> events for `sid` only.
    /// - `session_id: None`                            -> sessionless/global
    ///   events only. `include_global` is ignored.
    pub async fn replay_from(
        &self,
        from_event_seq: u64,
        filter: &EventFilter,
    ) -> Vec<EventEnvelope<CoreEvent>> {
        let ring_events = {
            let ring = self.ring.lock().await;
            ring.iter()
                .filter(|e| e.event_seq > from_event_seq)
                .filter(|e| filter_matches(filter, e))
                .cloned()
                .collect::<Vec<_>>()
        };

        // If ring buffer has events from the requested seq, use them
        if !ring_events.is_empty() {
            let earliest_seq = ring_events.first().map(|e| e.event_seq).unwrap_or(0);
            if earliest_seq <= from_event_seq.saturating_add(1) {
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

    /// Replay events strictly after `from_event_seq` from SQLite when
    /// the ring buffer doesn't have them. Returns events ordered by
    /// `event_seq` ASC.
    ///
    /// Filter semantics (mirrors the in-memory `filter_matches`):
    ///
    /// - `session_id: Some(sid), include_global: true`  -> `session_id = sid`
    ///   OR `session_id IS NULL` (global events).
    /// - `session_id: Some(sid), include_global: false` -> `session_id = sid`.
    /// - `session_id: None`                            -> `session_id IS NULL`
    ///   only. `include_global` is ignored.
    pub async fn replay_from_db(
        &self,
        from_event_seq: u64,
        filter: &EventFilter,
    ) -> Vec<EventEnvelope<CoreEvent>> {
        let Some(ref pool) = self.pool else {
            return Vec::new();
        };

        let bind_seq: i64 = from_event_seq as i64;
        let rows: Vec<CoreEventRow> = match (&filter.session_id, filter.include_global) {
            (Some(sid), true) => sqlx::query_as::<_, CoreEventRow>(
                "SELECT event_seq, session_id, turn_id, event_type, payload_json, created_at \
                     FROM core_event_log WHERE event_seq > ? \
                     AND (session_id = ? OR session_id IS NULL) ORDER BY event_seq ASC",
            )
            .bind(bind_seq)
            .bind(sid)
            .fetch_all(pool)
            .await
            .unwrap_or_default(),
            (Some(sid), false) => sqlx::query_as::<_, CoreEventRow>(
                "SELECT event_seq, session_id, turn_id, event_type, payload_json, created_at \
                     FROM core_event_log WHERE event_seq > ? AND session_id = ? \
                     ORDER BY event_seq ASC",
            )
            .bind(bind_seq)
            .bind(sid)
            .fetch_all(pool)
            .await
            .unwrap_or_default(),
            (None, _) => sqlx::query_as::<_, CoreEventRow>(
                "SELECT event_seq, session_id, turn_id, event_type, payload_json, created_at \
                     FROM core_event_log WHERE event_seq > ? AND session_id IS NULL \
                     ORDER BY event_seq ASC",
            )
            .bind(bind_seq)
            .fetch_all(pool)
            .await
            .unwrap_or_default(),
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

    /// Get the latest assigned sequence number. Returns 0 if no events
    /// have been published yet.
    pub fn current_seq(&self) -> u64 {
        self.next_seq.load(Ordering::SeqCst).saturating_sub(1)
    }

    /// Check if events exist strictly after `from_event_seq` (ring buffer or DB).
    /// `from_event_seq` is the last sequence the client has already seen; pass `0`
    /// to ask "do I have any events at all".
    pub async fn has_events_from(&self, from_event_seq: u64) -> bool {
        // Check ring buffer first. The ring is ordered by event_seq ascending,
        // so the back holds the highest seq; if the back is > from_event_seq
        // then the ring has at least one event with seq > from_event_seq.
        {
            let ring = self.ring.lock().await;
            if let Some(back) = ring.back() {
                if back.event_seq > from_event_seq {
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

    /// Check if the DB has events strictly after `from_event_seq`.
    pub async fn has_events_in_db(&self, from_event_seq: u64) -> bool {
        let Some(ref pool) = self.pool else {
            return false;
        };
        sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM core_event_log WHERE event_seq > ?")
            .bind(from_event_seq as i64)
            .fetch_one(pool)
            .await
            .map(|count| count > 0)
            .unwrap_or(false)
    }

    /// Returns true when the event log can replay events strictly after
    /// `from_event_seq` (i.e. the requested sequence is still covered by
    /// in-memory or persisted state). Returns true when `from_event_seq >=
    /// current_seq` (no new events) and when the ring buffer front still
    /// covers the next needed event. Falls through to the DB if the ring
    /// doesn't cover it and a pool is configured.
    ///
    /// This is the function the resume path uses to decide between
    /// `Events { events, current_seq }` (covered) and `ResyncRequired`
    /// (too old). A client already caught up (e.g. `from_event_seq ==
    /// current_seq`) is considered covered and gets an empty `Events`
    /// response.
    pub async fn covers_from(&self, from_event_seq: u64) -> bool {
        let current = self.current_seq();

        // No new events to deliver: trivially covered. The resume path
        // will return `Events { events: [], current_seq }` from this.
        if from_event_seq >= current {
            return true;
        }

        // The ring holds the contiguous oldest->newest stretch it has
        // seen; the front is the lowest seq. To replay events strictly
        // after `from_event_seq`, the first needed event has seq
        // `from_event_seq + 1`. If the front's seq is at or below that
        // boundary, the ring still covers the request.
        {
            let ring = self.ring.lock().await;
            if let Some(front) = ring.front() {
                if front.event_seq <= from_event_seq.saturating_add(1) {
                    return true;
                }
            }
        }

        if self.pool.is_some() {
            self.db_covers_from(from_event_seq).await
        } else {
            false
        }
    }

    /// DB-backed coverage check: the SQLite event log covers the request
    /// when its lowest stored seq is at or below `from_event_seq + 1` AND
    /// its highest stored seq is at or above `from_event_seq + 1`. A
    /// `from_event_seq >= current_seq` is handled by the caller.
    async fn db_covers_from(&self, from_event_seq: u64) -> bool {
        let Some(ref pool) = self.pool else {
            return false;
        };
        let row: Option<(Option<i64>, Option<i64>)> =
            sqlx::query_as("SELECT MIN(event_seq), MAX(event_seq) FROM core_event_log")
                .fetch_optional(pool)
                .await
                .ok()
                .flatten();
        if let Some((min_seq, max_seq)) = row {
            let need = (from_event_seq as i64).saturating_add(1);
            match (min_seq, max_seq) {
                (Some(lo), Some(hi)) => lo <= need && hi >= need,
                _ => false,
            }
        } else {
            false
        }
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

    /// Build a fresh in-memory SQLite pool with the full session schema.
    /// Used by tests that need a backing DB. No on-disk tempdir is
    /// created, so the pool's underlying memory is reclaimed when the
    /// test's `SqlitePool` is dropped — no `Box::leak` required.
    async fn in_memory_pool() -> sqlx::SqlitePool {
        use crate::session::schema::migrate;
        use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
        use std::str::FromStr;
        let url = format!(
            "file:eventlog_test_{}?mode=memory&cache=shared",
            uuid::Uuid::new_v4().simple()
        );
        let opts = SqliteConnectOptions::from_str(&url)
            .expect("valid sqlite options")
            .create_if_missing(true)
            .busy_timeout(std::time::Duration::from_secs(5))
            .foreign_keys(true);
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect_with(opts)
            .await
            .expect("connect in-memory sqlite");
        migrate(&pool).await.expect("migrate");
        pool
    }

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
                0,
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
    async fn replay_filters_by_envelope_session_id() {
        let log = EventLog::new(100);
        log.publish(
            Some("s1".into()),
            None,
            CoreEvent::Error {
                code: "session-event".into(),
                message: "m".into(),
            },
        )
        .await;
        log.publish(
            None,
            None,
            CoreEvent::Error {
                code: "global-event".into(),
                message: "m".into(),
            },
        )
        .await;

        let filter = EventFilter {
            session_id: Some("s1".into()),
            client_id: None,
            include_global: false,
        };
        let events = log.replay_from(0, &filter).await;
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].session_id.as_deref(), Some("s1"));
    }

    #[tokio::test]
    async fn replay_after_last_seen_does_not_duplicate() {
        let log = EventLog::new(100);
        let s1 = log
            .publish(
                Some("s1".into()),
                None,
                CoreEvent::Error {
                    code: "e1".into(),
                    message: "m".into(),
                },
            )
            .await;
        assert_eq!(s1, 1);

        let events = log
            .replay_from(
                s1,
                &EventFilter {
                    session_id: Some("s1".into()),
                    client_id: None,
                    include_global: false,
                },
            )
            .await;
        assert!(events.is_empty(), "expected no events, got {:?}", events);
    }

    #[tokio::test]
    async fn replay_from_zero_returns_first_event() {
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

        let events = log
            .replay_from(
                0,
                &EventFilter {
                    session_id: Some("s1".into()),
                    client_id: None,
                    include_global: false,
                },
            )
            .await;
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].event_seq, 1);
    }

    #[tokio::test]
    async fn replay_strictly_after_seq() {
        let log = EventLog::new(100);
        for i in 1..=3 {
            log.publish(
                Some("s1".into()),
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
                2,
                &EventFilter {
                    session_id: Some("s1".into()),
                    client_id: None,
                    include_global: false,
                },
            )
            .await;
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].event_seq, 3);
    }

    #[tokio::test]
    async fn replay_semantics_zero_one_two_with_two_events() {
        // Combined replay semantics test for Pass I of the integration
        // matrix: with two session events already published,
        // `replay_from(0, ...)` must return both, `replay_from(1, ...)`
        // must return only the second, and `replay_from(2, ...)` must
        // return none. We use a session-scoped filter
        // (`session_id: Some, include_global: true`) so it picks up
        // both events; a global-only filter would not, by design.
        let log = EventLog::new(100);
        let s1 = log
            .publish(
                Some("s1".into()),
                None,
                CoreEvent::Error {
                    code: "e1".into(),
                    message: "m1".into(),
                },
            )
            .await;
        let s2 = log
            .publish(
                Some("s1".into()),
                None,
                CoreEvent::Error {
                    code: "e2".into(),
                    message: "m2".into(),
                },
            )
            .await;
        assert_eq!(s1, 1);
        assert_eq!(s2, 2);

        let s1_filter = EventFilter {
            session_id: Some("s1".into()),
            client_id: None,
            include_global: true,
        };

        // resume from 0 -> both events
        let events = log.replay_from(0, &s1_filter).await;
        assert_eq!(events.len(), 2, "expected both events, got {:?}", events);
        assert_eq!(events[0].event_seq, 1);
        assert_eq!(events[1].event_seq, 2);

        // resume from 1 -> only the second event
        let events = log.replay_from(1, &s1_filter).await;
        assert_eq!(
            events.len(),
            1,
            "expected only second event, got {:?}",
            events
        );
        assert_eq!(events[0].event_seq, 2);

        // resume from 2 -> none
        let events = log.replay_from(2, &s1_filter).await;
        assert!(events.is_empty(), "expected no events, got {:?}", events);
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

    #[tokio::test]
    async fn has_events_from_strict_semantics() {
        let pool = in_memory_pool().await;

        let log = EventLog::new_with_pool(100, pool.clone());
        let s1 = log
            .publish(
                Some("s1".into()),
                None,
                CoreEvent::TurnStarted {
                    session_id: "s1".into(),
                    turn_id: "t1".into(),
                },
            )
            .await;
        assert_eq!(s1, 1);

        assert!(log.has_events_from(0).await);
        assert!(!log.has_events_from(1).await);
        assert!(log.has_events_in_db(0).await);
        assert!(!log.has_events_in_db(1).await);
    }

    #[tokio::test]
    async fn event_log_persists_event_type_as_string() {
        let pool = in_memory_pool().await;

        let log = EventLog::new_with_pool(100, pool.clone());
        log.publish(
            Some("s1".into()),
            Some("t1".into()),
            CoreEvent::TurnStarted {
                session_id: "s1".into(),
                turn_id: "t1".into(),
            },
        )
        .await;

        let row: (String,) =
            sqlx::query_as("SELECT event_type FROM core_event_log ORDER BY event_seq ASC LIMIT 1")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(row.0, "turn_started");
    }

    #[tokio::test]
    async fn covers_from_current_seq_is_true() {
        // An already-caught-up client is considered covered (empty replay).
        let log = EventLog::new(100);
        let s1 = log
            .publish(
                Some("s1".into()),
                None,
                CoreEvent::Error {
                    code: "e1".into(),
                    message: "m".into(),
                },
            )
            .await;
        assert!(log.covers_from(s1).await);
        // Future seqs are also covered.
        assert!(log.covers_from(s1 + 1).await);
        assert!(log.covers_from(999_999).await);
    }

    #[tokio::test]
    async fn covers_from_too_old_ring_seq_is_false_without_db() {
        // With a small ring and no DB, a too-old seq is NOT covered.
        let log = EventLog::new(3);
        for _ in 0..5 {
            log.publish(
                Some("s1".into()),
                None,
                CoreEvent::Error {
                    code: "e".into(),
                    message: "m".into(),
                },
            )
            .await;
        }
        // current_seq is 5, ring covers 3..=5. Asking for from_event_seq=0
        // means we need event 1, which the ring no longer has.
        assert!(!log.covers_from(0).await);
        // Asking for from_event_seq=2 means we need event 3, which the
        // ring still has (front of ring is 3).
        assert!(log.covers_from(2).await);
    }

    #[tokio::test]
    async fn covers_from_too_old_ring_seq_is_true_with_db() {
        // Same scenario, but with a SQLite pool: the DB still has
        // event 1 so the request is covered.
        let pool = in_memory_pool().await;

        let log = EventLog::new_with_pool(3, pool.clone());
        for i in 0..5 {
            log.publish(
                Some("s1".into()),
                None,
                CoreEvent::Error {
                    code: format!("e{}", i),
                    message: "m".into(),
                },
            )
            .await;
        }
        // current_seq is 5; persisted range is 1..=5 (only persisted
        // events show up in core_event_log; turn_started is the only
        // one that round-trips here, but the log treats the bounded
        // ring as authoritative for current_seq).
        // We can check at least that covers_from(0) is true thanks to
        // the DB layer.
        assert!(log.covers_from(0).await);
    }

    #[tokio::test]
    async fn replay_from_current_seq_returns_empty() {
        let log = EventLog::new(100);
        let s1 = log
            .publish(
                Some("s1".into()),
                None,
                CoreEvent::Error {
                    code: "e1".into(),
                    message: "m".into(),
                },
            )
            .await;
        let events = log
            .replay_from(
                s1,
                &EventFilter {
                    session_id: Some("s1".into()),
                    client_id: None,
                    include_global: true,
                },
            )
            .await;
        assert!(events.is_empty(), "expected empty replay, got {:?}", events);
    }
}

//! Durable control and stable-boundary journal for delegated agent runs.
//!
//! This module deliberately contains no UI or transport concerns.  The run
//! store remains the authority for ownership and terminal state; this store
//! owns the ordered mailbox and the bounded recovery journal that accompany a
//! run.  A daemon-side adapter may attach live channels after persistence.

use async_trait::async_trait;
use chrono::Utc;
use serde::{Deserialize, Serialize};
use sqlx::{Row, SqlitePool};
use std::collections::{BTreeMap, HashMap};
use thiserror::Error;
use tokio::sync::Mutex;

use crate::identity::{AgentRunId, AgentRunMessageId};

pub const MAX_CONTROL_PAYLOAD_BYTES: usize = 8 * 1024;
pub const MAX_JOURNAL_METADATA_BYTES: usize = 4 * 1024;
pub const MAX_PENDING_CONTROLS_PER_RUN: usize = 256;
pub const MAX_CONTROLS_PER_MINUTE: usize = 128;
pub const MAX_JOURNAL_EVENTS_PER_RUN: usize = 4096;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentRunControlKind {
    Message,
    Interrupt,
    Cancel,
}

impl AgentRunControlKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Message => "message",
            Self::Interrupt => "interrupt",
            Self::Cancel => "cancel",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MailboxState {
    Queued,
    Delivered,
    Acknowledged,
    Superseded,
}

impl MailboxState {
    fn parse(value: &str) -> Self {
        match value {
            "delivered" => Self::Delivered,
            "acknowledged" => Self::Acknowledged,
            "superseded" => Self::Superseded,
            _ => Self::Queued,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentRunMailboxMessage {
    pub message_id: AgentRunMessageId,
    pub run_id: AgentRunId,
    pub sender_run_id: Option<AgentRunId>,
    pub kind: AgentRunControlKind,
    pub payload: String,
    pub sequence: u64,
    pub state: MailboxState,
    pub idempotency_key: String,
    pub causation_id: Option<String>,
    pub created_at: i64,
    pub delivered_at: Option<i64>,
    pub acknowledged_at: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentRunJournalEventKind {
    RunCreated,
    RunQueued,
    RunStarted,
    ControlQueued,
    ControlDelivered,
    SafeBoundary,
    ProgressMilestone,
    CancelRequested,
    CompletionProduced,
    RecoveryTransition,
}

impl AgentRunJournalEventKind {
    fn as_str(&self) -> &'static str {
        match self {
            Self::RunCreated => "run_created",
            Self::RunQueued => "run_queued",
            Self::RunStarted => "run_started",
            Self::ControlQueued => "control_queued",
            Self::ControlDelivered => "control_delivered",
            Self::SafeBoundary => "safe_boundary",
            Self::ProgressMilestone => "progress_milestone",
            Self::CancelRequested => "cancel_requested",
            Self::CompletionProduced => "completion_produced",
            Self::RecoveryTransition => "recovery_transition",
        }
    }

    fn parse(value: &str) -> Self {
        match value {
            "run_created" => Self::RunCreated,
            "run_queued" => Self::RunQueued,
            "run_started" => Self::RunStarted,
            "control_queued" => Self::ControlQueued,
            "control_delivered" => Self::ControlDelivered,
            "safe_boundary" => Self::SafeBoundary,
            "progress_milestone" => Self::ProgressMilestone,
            "cancel_requested" => Self::CancelRequested,
            "recovery_transition" => Self::RecoveryTransition,
            _ => Self::CompletionProduced,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentRunJournalEvent {
    pub event_id: AgentRunMessageId,
    pub run_id: AgentRunId,
    pub sequence: u64,
    pub kind: AgentRunJournalEventKind,
    pub causation_id: Option<String>,
    pub correlation_id: Option<String>,
    pub metadata: BTreeMap<String, String>,
    pub created_at: i64,
}

#[derive(Debug, Clone)]
pub struct NewControlMessage {
    pub message_id: AgentRunMessageId,
    pub run_id: AgentRunId,
    pub sender_run_id: Option<AgentRunId>,
    pub kind: AgentRunControlKind,
    pub payload: String,
    pub idempotency_key: String,
    pub causation_id: Option<String>,
}

#[derive(Debug, Clone)]
pub struct NewJournalEvent {
    pub event_id: AgentRunMessageId,
    pub run_id: AgentRunId,
    pub kind: AgentRunJournalEventKind,
    pub causation_id: Option<String>,
    pub correlation_id: Option<String>,
    pub metadata: BTreeMap<String, String>,
}

#[derive(Debug, Error)]
pub enum AgentRunControlStoreError {
    #[error("storage failure: {0}")]
    Storage(String),
    #[error("control payload exceeds the {MAX_CONTROL_PAYLOAD_BYTES}-byte limit")]
    PayloadTooLarge,
    #[error("journal metadata exceeds the {MAX_JOURNAL_METADATA_BYTES}-byte limit")]
    MetadataTooLarge,
    #[error("mailbox capacity exceeded")]
    MailboxFull,
    #[error("control rate limit exceeded")]
    RateLimited,
    #[error("mailbox message '{0}' not found")]
    MessageNotFound(String),
    #[error("invalid mailbox transition: {from:?} -> {to:?}")]
    InvalidMailboxTransition {
        from: MailboxState,
        to: MailboxState,
    },
    #[error("serialization failure: {0}")]
    Serialization(String),
}

fn now() -> i64 {
    Utc::now().timestamp_millis()
}

fn validate_control(input: &NewControlMessage) -> Result<(), AgentRunControlStoreError> {
    if input.payload.len() > MAX_CONTROL_PAYLOAD_BYTES {
        return Err(AgentRunControlStoreError::PayloadTooLarge);
    }
    if input.idempotency_key.is_empty() || input.idempotency_key.len() > 256 {
        return Err(AgentRunControlStoreError::Storage(
            "idempotency key must be between 1 and 256 bytes".into(),
        ));
    }
    Ok(())
}

fn validate_event(input: &NewJournalEvent) -> Result<String, AgentRunControlStoreError> {
    let encoded = serde_json::to_vec(&input.metadata)
        .map_err(|e| AgentRunControlStoreError::Serialization(e.to_string()))?;
    if encoded.len() > MAX_JOURNAL_METADATA_BYTES {
        return Err(AgentRunControlStoreError::MetadataTooLarge);
    }
    Ok(String::from_utf8(encoded).unwrap_or_else(|_| "{}".into()))
}

#[async_trait]
pub trait AgentRunControlStore: Send + Sync {
    async fn enqueue(
        &self,
        input: NewControlMessage,
    ) -> Result<AgentRunMailboxMessage, AgentRunControlStoreError>;
    async fn pending(
        &self,
        run_id: &AgentRunId,
    ) -> Result<Vec<AgentRunMailboxMessage>, AgentRunControlStoreError>;
    async fn mark_delivered(
        &self,
        message_id: &AgentRunMessageId,
    ) -> Result<AgentRunMailboxMessage, AgentRunControlStoreError>;
    async fn acknowledge(
        &self,
        message_id: &AgentRunMessageId,
    ) -> Result<AgentRunMailboxMessage, AgentRunControlStoreError>;
    async fn supersede(
        &self,
        message_id: &AgentRunMessageId,
    ) -> Result<AgentRunMailboxMessage, AgentRunControlStoreError>;
    async fn append_event(
        &self,
        input: NewJournalEvent,
    ) -> Result<AgentRunJournalEvent, AgentRunControlStoreError>;
    async fn list_events(
        &self,
        run_id: &AgentRunId,
        after_sequence: u64,
        limit: usize,
    ) -> Result<Vec<AgentRunJournalEvent>, AgentRunControlStoreError>;
    async fn pending_count(&self, run_id: &AgentRunId) -> Result<usize, AgentRunControlStoreError>;
}

#[derive(Default)]
struct MemoryState {
    messages: HashMap<AgentRunMessageId, AgentRunMailboxMessage>,
    by_key: HashMap<(AgentRunId, String), AgentRunMessageId>,
    next_message_sequence: HashMap<AgentRunId, u64>,
    events: HashMap<AgentRunId, Vec<AgentRunJournalEvent>>,
}

#[derive(Default)]
pub struct InMemoryAgentRunControlStore {
    state: Mutex<MemoryState>,
}

impl InMemoryAgentRunControlStore {
    pub fn new() -> Self {
        Self::default()
    }
}

#[async_trait]
impl AgentRunControlStore for InMemoryAgentRunControlStore {
    async fn enqueue(
        &self,
        input: NewControlMessage,
    ) -> Result<AgentRunMailboxMessage, AgentRunControlStoreError> {
        validate_control(&input)?;
        let mut state = self.state.lock().await;
        if let Some(existing) = state
            .by_key
            .get(&(input.run_id.clone(), input.idempotency_key.clone()))
            .and_then(|id| state.messages.get(id))
        {
            return Ok(existing.clone());
        }
        let pending = state
            .messages
            .values()
            .filter(|m| {
                m.run_id == input.run_id
                    && matches!(m.state, MailboxState::Queued | MailboxState::Delivered)
            })
            .count();
        if pending >= MAX_PENDING_CONTROLS_PER_RUN {
            return Err(AgentRunControlStoreError::MailboxFull);
        }
        let current = now();
        let recent = state
            .messages
            .values()
            .filter(|m| m.run_id == input.run_id && current.saturating_sub(m.created_at) < 60_000)
            .count();
        if recent >= MAX_CONTROLS_PER_MINUTE {
            return Err(AgentRunControlStoreError::RateLimited);
        }
        let sequence = state
            .next_message_sequence
            .entry(input.run_id.clone())
            .or_insert(0);
        *sequence += 1;
        let message = AgentRunMailboxMessage {
            message_id: input.message_id.clone(),
            run_id: input.run_id.clone(),
            sender_run_id: input.sender_run_id,
            kind: input.kind,
            payload: input.payload,
            sequence: *sequence,
            state: MailboxState::Queued,
            idempotency_key: input.idempotency_key.clone(),
            causation_id: input.causation_id,
            created_at: current,
            delivered_at: None,
            acknowledged_at: None,
        };
        state.by_key.insert(
            (input.run_id, input.idempotency_key),
            input.message_id.clone(),
        );
        state.messages.insert(input.message_id, message.clone());
        Ok(message)
    }

    async fn pending(
        &self,
        run_id: &AgentRunId,
    ) -> Result<Vec<AgentRunMailboxMessage>, AgentRunControlStoreError> {
        let state = self.state.lock().await;
        let mut messages: Vec<_> = state
            .messages
            .values()
            .filter(|m| {
                &m.run_id == run_id
                    && matches!(m.state, MailboxState::Queued | MailboxState::Delivered)
            })
            .cloned()
            .collect();
        messages.sort_by_key(|m| m.sequence);
        Ok(messages)
    }

    async fn mark_delivered(
        &self,
        message_id: &AgentRunMessageId,
    ) -> Result<AgentRunMailboxMessage, AgentRunControlStoreError> {
        let mut state = self.state.lock().await;
        let message = state
            .messages
            .get_mut(message_id)
            .ok_or_else(|| AgentRunControlStoreError::MessageNotFound(message_id.to_string()))?;
        if message.state == MailboxState::Acknowledged || message.state == MailboxState::Delivered {
            return Ok(message.clone());
        }
        if message.state != MailboxState::Queued {
            return Err(AgentRunControlStoreError::InvalidMailboxTransition {
                from: message.state,
                to: MailboxState::Delivered,
            });
        }
        message.state = MailboxState::Delivered;
        message.delivered_at = Some(now());
        Ok(message.clone())
    }

    async fn acknowledge(
        &self,
        message_id: &AgentRunMessageId,
    ) -> Result<AgentRunMailboxMessage, AgentRunControlStoreError> {
        let mut state = self.state.lock().await;
        let message = state
            .messages
            .get_mut(message_id)
            .ok_or_else(|| AgentRunControlStoreError::MessageNotFound(message_id.to_string()))?;
        if message.state == MailboxState::Acknowledged {
            return Ok(message.clone());
        }
        if message.state != MailboxState::Delivered {
            return Err(AgentRunControlStoreError::InvalidMailboxTransition {
                from: message.state,
                to: MailboxState::Acknowledged,
            });
        }
        message.state = MailboxState::Acknowledged;
        message.acknowledged_at = Some(now());
        Ok(message.clone())
    }

    async fn supersede(
        &self,
        message_id: &AgentRunMessageId,
    ) -> Result<AgentRunMailboxMessage, AgentRunControlStoreError> {
        let mut state = self.state.lock().await;
        let message = state
            .messages
            .get_mut(message_id)
            .ok_or_else(|| AgentRunControlStoreError::MessageNotFound(message_id.to_string()))?;
        if matches!(
            message.state,
            MailboxState::Superseded | MailboxState::Acknowledged
        ) {
            return Ok(message.clone());
        }
        if !matches!(
            message.state,
            MailboxState::Queued | MailboxState::Delivered
        ) {
            return Err(AgentRunControlStoreError::InvalidMailboxTransition {
                from: message.state,
                to: MailboxState::Superseded,
            });
        }
        message.state = MailboxState::Superseded;
        Ok(message.clone())
    }

    async fn append_event(
        &self,
        input: NewJournalEvent,
    ) -> Result<AgentRunJournalEvent, AgentRunControlStoreError> {
        let metadata_json = validate_event(&input)?;
        let mut state = self.state.lock().await;
        let events = state.events.entry(input.run_id.clone()).or_default();
        if events.len() >= MAX_JOURNAL_EVENTS_PER_RUN {
            return Err(AgentRunControlStoreError::Storage(
                "journal retention limit reached".into(),
            ));
        }
        let event = AgentRunJournalEvent {
            event_id: input.event_id,
            run_id: input.run_id,
            sequence: events.len() as u64 + 1,
            kind: input.kind,
            causation_id: input.causation_id,
            correlation_id: input.correlation_id,
            metadata: serde_json::from_str(&metadata_json)
                .map_err(|e| AgentRunControlStoreError::Serialization(e.to_string()))?,
            created_at: now(),
        };
        events.push(event.clone());
        Ok(event)
    }

    async fn list_events(
        &self,
        run_id: &AgentRunId,
        after_sequence: u64,
        limit: usize,
    ) -> Result<Vec<AgentRunJournalEvent>, AgentRunControlStoreError> {
        let state = self.state.lock().await;
        Ok(state
            .events
            .get(run_id)
            .map(|events| {
                events
                    .iter()
                    .filter(|event| event.sequence > after_sequence)
                    .take(limit.min(MAX_JOURNAL_EVENTS_PER_RUN))
                    .cloned()
                    .collect()
            })
            .unwrap_or_default())
    }

    async fn pending_count(&self, run_id: &AgentRunId) -> Result<usize, AgentRunControlStoreError> {
        Ok(self.pending(run_id).await?.len())
    }
}

pub struct SqliteAgentRunControlStore {
    pool: SqlitePool,
    write_lock: Mutex<()>,
}

impl SqliteAgentRunControlStore {
    pub fn new(pool: SqlitePool) -> Self {
        Self {
            pool,
            write_lock: Mutex::new(()),
        }
    }
}

fn storage_error(error: sqlx::Error) -> AgentRunControlStoreError {
    AgentRunControlStoreError::Storage(error.to_string())
}

fn message_from_row(
    row: &sqlx::sqlite::SqliteRow,
) -> Result<AgentRunMailboxMessage, AgentRunControlStoreError> {
    Ok(AgentRunMailboxMessage {
        message_id: AgentRunMessageId::parse(row.get::<String, _>("message_id").as_str())
            .map_err(|e| AgentRunControlStoreError::Storage(e.to_string()))?,
        run_id: AgentRunId::parse(row.get::<String, _>("run_id").as_str())
            .map_err(|e| AgentRunControlStoreError::Storage(e.to_string()))?,
        sender_run_id: row
            .get::<Option<String>, _>("sender_run_id")
            .map(|value| AgentRunId::parse(&value))
            .transpose()
            .map_err(|e| AgentRunControlStoreError::Storage(e.to_string()))?,
        kind: match row.get::<String, _>("kind").as_str() {
            "interrupt" => AgentRunControlKind::Interrupt,
            "cancel" => AgentRunControlKind::Cancel,
            _ => AgentRunControlKind::Message,
        },
        payload: row.get("payload"),
        sequence: row.get::<i64, _>("sequence") as u64,
        state: MailboxState::parse(row.get::<String, _>("state").as_str()),
        idempotency_key: row.get("idempotency_key"),
        causation_id: row.get("causation_id"),
        created_at: row.get("created_at"),
        delivered_at: row.get("delivered_at"),
        acknowledged_at: row.get("acknowledged_at"),
    })
}

const MESSAGE_COLUMNS: &str = "message_id, run_id, sender_run_id, kind, payload, sequence, state, idempotency_key, causation_id, created_at, delivered_at, acknowledged_at";

#[async_trait]
impl AgentRunControlStore for SqliteAgentRunControlStore {
    async fn enqueue(
        &self,
        input: NewControlMessage,
    ) -> Result<AgentRunMailboxMessage, AgentRunControlStoreError> {
        validate_control(&input)?;
        let _guard = self.write_lock.lock().await;
        if let Some(row) = sqlx::query(&format!("SELECT {MESSAGE_COLUMNS} FROM agent_run_mailbox WHERE run_id = ? AND idempotency_key = ?"))
            .bind(input.run_id.as_str()).bind(&input.idempotency_key).fetch_optional(&self.pool).await.map_err(storage_error)? {
            return message_from_row(&row);
        }
        let pending: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM agent_run_mailbox WHERE run_id = ? AND state IN ('queued','delivered')")
            .bind(input.run_id.as_str()).fetch_one(&self.pool).await.map_err(storage_error)?;
        if pending as usize >= MAX_PENDING_CONTROLS_PER_RUN {
            return Err(AgentRunControlStoreError::MailboxFull);
        }
        let cutoff = now() - 60_000;
        let recent: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM agent_run_mailbox WHERE run_id = ? AND created_at >= ?",
        )
        .bind(input.run_id.as_str())
        .bind(cutoff)
        .fetch_one(&self.pool)
        .await
        .map_err(storage_error)?;
        if recent as usize >= MAX_CONTROLS_PER_MINUTE {
            return Err(AgentRunControlStoreError::RateLimited);
        }
        let sequence: i64 = sqlx::query_scalar(
            "SELECT COALESCE(MAX(sequence), 0) + 1 FROM agent_run_mailbox WHERE run_id = ?",
        )
        .bind(input.run_id.as_str())
        .fetch_one(&self.pool)
        .await
        .map_err(storage_error)?;
        let created_at = now();
        sqlx::query("INSERT INTO agent_run_mailbox (message_id, run_id, sender_run_id, kind, payload, sequence, state, idempotency_key, causation_id, created_at) VALUES (?, ?, ?, ?, ?, ?, 'queued', ?, ?, ?)")
            .bind(input.message_id.as_str()).bind(input.run_id.as_str()).bind(input.sender_run_id.as_ref().map(AgentRunId::as_str)).bind(input.kind.as_str()).bind(&input.payload).bind(sequence).bind(&input.idempotency_key).bind(input.causation_id.as_deref()).bind(created_at).execute(&self.pool).await.map_err(storage_error)?;
        let row = sqlx::query(&format!(
            "SELECT {MESSAGE_COLUMNS} FROM agent_run_mailbox WHERE message_id = ?"
        ))
        .bind(input.message_id.as_str())
        .fetch_one(&self.pool)
        .await
        .map_err(storage_error)?;
        message_from_row(&row)
    }

    async fn pending(
        &self,
        run_id: &AgentRunId,
    ) -> Result<Vec<AgentRunMailboxMessage>, AgentRunControlStoreError> {
        let rows = sqlx::query(&format!("SELECT {MESSAGE_COLUMNS} FROM agent_run_mailbox WHERE run_id = ? AND state IN ('queued','delivered') ORDER BY sequence ASC LIMIT {}", MAX_PENDING_CONTROLS_PER_RUN))
            .bind(run_id.as_str()).fetch_all(&self.pool).await.map_err(storage_error)?;
        rows.iter().map(message_from_row).collect()
    }

    async fn mark_delivered(
        &self,
        message_id: &AgentRunMessageId,
    ) -> Result<AgentRunMailboxMessage, AgentRunControlStoreError> {
        let _guard = self.write_lock.lock().await;
        let updated = sqlx::query("UPDATE agent_run_mailbox SET state = 'delivered', delivered_at = ? WHERE message_id = ? AND state = 'queued'")
            .bind(now()).bind(message_id.as_str()).execute(&self.pool).await.map_err(storage_error)?;
        if updated.rows_affected() == 0 {
            let row = sqlx::query(&format!(
                "SELECT {MESSAGE_COLUMNS} FROM agent_run_mailbox WHERE message_id = ?"
            ))
            .bind(message_id.as_str())
            .fetch_optional(&self.pool)
            .await
            .map_err(storage_error)?;
            let Some(row) = row else {
                return Err(AgentRunControlStoreError::MessageNotFound(
                    message_id.to_string(),
                ));
            };
            let message = message_from_row(&row)?;
            if matches!(
                message.state,
                MailboxState::Delivered | MailboxState::Acknowledged
            ) {
                return Ok(message);
            }
            return Err(AgentRunControlStoreError::InvalidMailboxTransition {
                from: message.state,
                to: MailboxState::Delivered,
            });
        }
        let row = sqlx::query(&format!(
            "SELECT {MESSAGE_COLUMNS} FROM agent_run_mailbox WHERE message_id = ?"
        ))
        .bind(message_id.as_str())
        .fetch_one(&self.pool)
        .await
        .map_err(storage_error)?;
        message_from_row(&row)
    }

    async fn acknowledge(
        &self,
        message_id: &AgentRunMessageId,
    ) -> Result<AgentRunMailboxMessage, AgentRunControlStoreError> {
        let _guard = self.write_lock.lock().await;
        let updated = sqlx::query("UPDATE agent_run_mailbox SET state = 'acknowledged', acknowledged_at = ? WHERE message_id = ? AND state = 'delivered'")
            .bind(now()).bind(message_id.as_str()).execute(&self.pool).await.map_err(storage_error)?;
        let row = sqlx::query(&format!(
            "SELECT {MESSAGE_COLUMNS} FROM agent_run_mailbox WHERE message_id = ?"
        ))
        .bind(message_id.as_str())
        .fetch_optional(&self.pool)
        .await
        .map_err(storage_error)?;
        let Some(row) = row else {
            return Err(AgentRunControlStoreError::MessageNotFound(
                message_id.to_string(),
            ));
        };
        let message = message_from_row(&row)?;
        if updated.rows_affected() == 0 && message.state != MailboxState::Acknowledged {
            return Err(AgentRunControlStoreError::InvalidMailboxTransition {
                from: message.state,
                to: MailboxState::Acknowledged,
            });
        }
        Ok(message)
    }

    async fn supersede(
        &self,
        message_id: &AgentRunMessageId,
    ) -> Result<AgentRunMailboxMessage, AgentRunControlStoreError> {
        let _guard = self.write_lock.lock().await;
        let updated = sqlx::query("UPDATE agent_run_mailbox SET state = 'superseded' WHERE message_id = ? AND state IN ('queued','delivered')")
            .bind(message_id.as_str())
            .execute(&self.pool)
            .await
            .map_err(storage_error)?;
        let row = sqlx::query(&format!(
            "SELECT {MESSAGE_COLUMNS} FROM agent_run_mailbox WHERE message_id = ?"
        ))
        .bind(message_id.as_str())
        .fetch_optional(&self.pool)
        .await
        .map_err(storage_error)?;
        let Some(row) = row else {
            return Err(AgentRunControlStoreError::MessageNotFound(
                message_id.to_string(),
            ));
        };
        let message = message_from_row(&row)?;
        if updated.rows_affected() == 0
            && !matches!(
                message.state,
                MailboxState::Superseded | MailboxState::Acknowledged
            )
        {
            return Err(AgentRunControlStoreError::InvalidMailboxTransition {
                from: message.state,
                to: MailboxState::Superseded,
            });
        }
        Ok(message)
    }

    async fn append_event(
        &self,
        input: NewJournalEvent,
    ) -> Result<AgentRunJournalEvent, AgentRunControlStoreError> {
        let metadata_json = validate_event(&input)?;
        let _guard = self.write_lock.lock().await;
        let count: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM agent_run_journal WHERE run_id = ?")
                .bind(input.run_id.as_str())
                .fetch_one(&self.pool)
                .await
                .map_err(storage_error)?;
        if count as usize >= MAX_JOURNAL_EVENTS_PER_RUN {
            return Err(AgentRunControlStoreError::Storage(
                "journal retention limit reached".into(),
            ));
        }
        let sequence = count + 1;
        let created_at = now();
        sqlx::query("INSERT INTO agent_run_journal (event_id, run_id, sequence, kind, causation_id, correlation_id, metadata_json, created_at) VALUES (?, ?, ?, ?, ?, ?, ?, ?)")
            .bind(input.event_id.as_str()).bind(input.run_id.as_str()).bind(sequence).bind(input.kind.as_str()).bind(input.causation_id.as_deref()).bind(input.correlation_id.as_deref()).bind(&metadata_json).bind(created_at).execute(&self.pool).await.map_err(storage_error)?;
        Ok(AgentRunJournalEvent {
            event_id: input.event_id,
            run_id: input.run_id,
            sequence: sequence as u64,
            kind: input.kind,
            causation_id: input.causation_id,
            correlation_id: input.correlation_id,
            metadata: serde_json::from_str(&metadata_json)
                .map_err(|e| AgentRunControlStoreError::Serialization(e.to_string()))?,
            created_at,
        })
    }

    async fn list_events(
        &self,
        run_id: &AgentRunId,
        after_sequence: u64,
        limit: usize,
    ) -> Result<Vec<AgentRunJournalEvent>, AgentRunControlStoreError> {
        let rows = sqlx::query("SELECT event_id, run_id, sequence, kind, causation_id, correlation_id, metadata_json, created_at FROM agent_run_journal WHERE run_id = ? AND sequence > ? ORDER BY sequence ASC LIMIT ?")
            .bind(run_id.as_str()).bind(after_sequence as i64).bind(limit.min(MAX_JOURNAL_EVENTS_PER_RUN) as i64).fetch_all(&self.pool).await.map_err(storage_error)?;
        rows.into_iter()
            .map(|row| {
                Ok(AgentRunJournalEvent {
                    event_id: AgentRunMessageId::parse(row.get::<String, _>("event_id").as_str())
                        .map_err(|e| AgentRunControlStoreError::Storage(e.to_string()))?,
                    run_id: AgentRunId::parse(row.get::<String, _>("run_id").as_str())
                        .map_err(|e| AgentRunControlStoreError::Storage(e.to_string()))?,
                    sequence: row.get::<i64, _>("sequence") as u64,
                    kind: AgentRunJournalEventKind::parse(row.get::<String, _>("kind").as_str()),
                    causation_id: row.get("causation_id"),
                    correlation_id: row.get("correlation_id"),
                    metadata: serde_json::from_str(row.get::<String, _>("metadata_json").as_str())
                        .map_err(|e| AgentRunControlStoreError::Serialization(e.to_string()))?,
                    created_at: row.get("created_at"),
                })
            })
            .collect()
    }

    async fn pending_count(&self, run_id: &AgentRunId) -> Result<usize, AgentRunControlStoreError> {
        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM agent_run_mailbox WHERE run_id = ? AND state IN ('queued','delivered')").bind(run_id.as_str()).fetch_one(&self.pool).await.map_err(storage_error)?;
        Ok(count as usize)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::identity::AgentRunId;

    fn control(run_id: &AgentRunId, key: &str, payload: &str) -> NewControlMessage {
        NewControlMessage {
            message_id: AgentRunMessageId::new(),
            run_id: run_id.clone(),
            sender_run_id: None,
            kind: AgentRunControlKind::Message,
            payload: payload.into(),
            idempotency_key: key.into(),
            causation_id: None,
        }
    }

    #[tokio::test]
    async fn mailbox_is_ordered_and_idempotent() {
        let store = InMemoryAgentRunControlStore::new();
        let run = AgentRunId::new();
        let first = store.enqueue(control(&run, "one", "a")).await.unwrap();
        let duplicate = store
            .enqueue(control(&run, "one", "different"))
            .await
            .unwrap();
        assert_eq!(first, duplicate);
        let second = store.enqueue(control(&run, "two", "b")).await.unwrap();
        assert_eq!((first.sequence, second.sequence), (1, 2));
        assert_eq!(store.pending(&run).await.unwrap().len(), 2);
    }

    #[tokio::test]
    async fn concurrent_senders_get_one_deterministic_sequence() {
        let store = std::sync::Arc::new(InMemoryAgentRunControlStore::new());
        let run = AgentRunId::new();
        let mut tasks = Vec::new();
        for index in 0..16 {
            let store = store.clone();
            let run = run.clone();
            tasks.push(tokio::spawn(async move {
                store
                    .enqueue(control(&run, &format!("key-{index}"), "payload"))
                    .await
                    .unwrap()
            }));
        }
        for task in tasks {
            task.await.unwrap();
        }
        let messages = store.pending(&run).await.unwrap();
        assert_eq!(
            messages.iter().map(|m| m.sequence).collect::<Vec<_>>(),
            (1..=16).collect::<Vec<_>>()
        );
    }

    #[tokio::test]
    async fn payload_and_transition_bounds_are_enforced() {
        let store = InMemoryAgentRunControlStore::new();
        let run = AgentRunId::new();
        let mut too_large = control(&run, "large", "x");
        too_large.payload = "x".repeat(MAX_CONTROL_PAYLOAD_BYTES + 1);
        assert!(matches!(
            store.enqueue(too_large).await,
            Err(AgentRunControlStoreError::PayloadTooLarge)
        ));
        let message = store.enqueue(control(&run, "ok", "safe")).await.unwrap();
        assert!(store.acknowledge(&message.message_id).await.is_err());
        store.mark_delivered(&message.message_id).await.unwrap();
        store.acknowledge(&message.message_id).await.unwrap();
        assert!(store.pending(&run).await.unwrap().is_empty());
    }
}

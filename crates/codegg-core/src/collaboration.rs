//! Project-scoped channels, messages, and synchronization (Collaboration M001).
//!
//! The daemon owns one durable message sequence per project channel plus
//! idempotent message IDs. Messages carry bounded text and structured
//! references to CodeGG objects; large content uses handles/artifacts owned
//! elsewhere. Typing/composing is ephemeral presence-like state; read
//! markers are principal-scoped durable lightweight state.
//!
//! ## Design notes
//!
//! - Project authorization guards every operation at the daemon boundary
//!   through `project.chat`; this module performs no authorization itself
//!   so the daemon gate stays the single authority. Unknown channels fail
//!   closed via [`channel_project`] returning `None`.
//! - Message IDs are opaque [`ChatMessageId`] values. Client retries carry
//!   an idempotency key scoped to `(channel_id, key)`: a duplicate send
//!   returns the original message and never assigns a second sequence
//!   number.
//! - Ordering is deterministic: `seq` is assigned inside one SQLite
//!   transaction as `MAX(seq) + 1` per channel, with a `UNIQUE` backstop
//!   and one retry on contention. Concurrent sends converge on distinct
//!   sequences.
//! - Edits and redactions bump `revision` with optimistic-concurrency
//!   checks. Every revision is preserved in `chat_revision` so the
//!   append-only history survives; only the author may edit or redact
//!   their own message in this milestone.
//! - Free text never executes: bodies are inert strings. There is no
//!   command parsing, mention-triggered dispatch, or model inference on
//!   the store path.
//! - Secrets are redacted at the boundary: [`redact_secrets_in_body`]
//!   replaces credential-like values with `[REDACTED]` before durable
//!   write, and reference display hints are truncated and scanned the
//!   same way.
//! - Composing state is ephemeral and in-memory only ([`ComposingState`]).
//!   Daemon restart drops it via a fresh service; restart never fabricates
//!   durable messages.
//! - Retention is explicit: each channel keeps at most
//!   `max_messages_per_channel` newest messages. Sync cursors at or above
//!   the retention floor resume incrementally; older cursors receive a
//!   bounded resync page.
//!
//! ## Transport contract
//!
//! Wire DTOs live in [`codegg_protocol::core`] (`ChatMessageDto`,
//! `ChatChannelDto`, `ChatCapabilitiesDto`, `ChatComposingDto`,
//! `ChatReferenceDto`). This module owns the domain state machine and
//! converts to those DTOs. Chat storage remains separate from the
//! append-only audit store; [`audit_metadata_for_message`] exposes only
//! structural locators (never bodies) for future audit linkage.

use std::sync::Arc;
use std::time::{Duration, Instant};

use dashmap::DashMap;
use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;
use thiserror::Error;

use crate::error::StorageError;
use crate::identity::{ChannelId, ChatMessageId, PrincipalId, ProjectId};

mod store;
mod validation;
pub use store::{
    channel_project, ensure_collaboration_tables, CHAT_ACTION_SCHEMA_STATEMENTS,
    CHAT_SCHEMA_STATEMENTS,
};
use store::{
    encode_mentions, encode_references, row_to_channel, row_to_message, ActionRow, MessageRow,
};
pub use validation::{
    audit_metadata_for_action, audit_metadata_for_message, redact_secrets_in_body,
    validate_action_agent, validate_action_job_id, validate_action_prompt, validate_action_title,
    validate_action_workspace, validate_idempotency_key, validate_idempotency_key_for_action,
};
use validation::{
    validate_author_agent, validate_body, validate_channel_name, validate_mentions,
    validate_references,
};

/// Version of the project-chat protocol surface.
pub const CHAT_PROTOCOL_VERSION: u32 = 1;
/// Capability string advertised for the chat surface.
pub const CHAT_CAPABILITY: &str = "chat.v1";
/// Marker placed where credential-like material was removed.
pub const REDACTED_MARKER: &str = "[REDACTED]";
/// Stored body for a redacted message.
pub const REDACTED_BODY: &str = "[REDACTED]";
/// Default channel name created for a project with no channels yet.
pub const DEFAULT_CHANNEL_NAME: &str = "general";

/// Bounds for the durable chat tables and the ephemeral composing map.
#[derive(Debug, Clone)]
pub struct CollaborationConfig {
    /// Maximum UTF-8 bytes accepted for one message body.
    pub max_body_bytes: usize,
    /// Maximum mentions accepted per message.
    pub max_mentions: usize,
    /// Maximum structured references accepted per message.
    pub max_references: usize,
    /// Maximum UTF-8 bytes accepted for a channel name.
    pub max_channel_name_len: usize,
    /// Maximum channels retained per project.
    pub max_channels_per_project: usize,
    /// Maximum messages retained per channel (retention window).
    pub max_messages_per_channel: usize,
    /// Maximum page size served by history/sync.
    pub max_page_limit: u32,
    /// Default page size when the caller passes no limit.
    pub default_page_limit: u32,
    /// How long a composing lease survives without renewal.
    pub composing_ttl: Duration,
    /// Maximum UTF-8 bytes accepted for an idempotency key.
    pub max_idempotency_key_len: usize,
    /// Maximum UTF-8 bytes accepted for an author-agent label.
    pub max_author_agent_len: usize,
    /// M003: maximum UTF-8 bytes accepted for a chat-action title.
    pub max_action_title_bytes: usize,
    /// M003: maximum UTF-8 bytes accepted for an agent-task prompt.
    pub max_action_prompt_bytes: usize,
    /// M003: maximum UTF-8 bytes accepted for an agent name.
    pub max_action_agent_len: usize,
    /// M003: maximum actions listed in one page.
    pub max_action_page_limit: u32,
}

impl Default for CollaborationConfig {
    fn default() -> Self {
        Self {
            max_body_bytes: 8 * 1024,
            max_mentions: 16,
            max_references: 8,
            max_channel_name_len: 64,
            max_channels_per_project: 16,
            max_messages_per_channel: 1000,
            max_page_limit: 100,
            default_page_limit: 50,
            composing_ttl: Duration::from_secs(30),
            max_idempotency_key_len: 128,
            max_author_agent_len: 128,
            max_action_title_bytes: 512,
            max_action_prompt_bytes: 8 * 1024,
            max_action_agent_len: 128,
            max_action_page_limit: 100,
        }
    }
}

impl CollaborationConfig {
    /// Wire capability advertisement for this configuration.
    pub fn capabilities_dto(&self) -> codegg_protocol::core::ChatCapabilitiesDto {
        codegg_protocol::core::ChatCapabilitiesDto {
            supported: true,
            protocol_version: CHAT_PROTOCOL_VERSION,
            max_body_bytes: self.max_body_bytes,
            max_page_limit: self.max_page_limit,
            max_mentions: self.max_mentions,
            max_references: self.max_references,
            composing_ttl_secs: self.composing_ttl.as_secs(),
            retention_max_messages: self.max_messages_per_channel,
            actions_supported: true,
            max_action_title_bytes: self.max_action_title_bytes,
            max_action_prompt_bytes: self.max_action_prompt_bytes,
        }
    }

    fn clamp_page_limit(&self, requested: Option<u32>) -> u32 {
        let limit = requested.unwrap_or(self.default_page_limit).max(1);
        limit.min(self.max_page_limit)
    }

    fn clamp_action_page_limit(&self, requested: Option<u32>) -> u32 {
        let limit = requested.unwrap_or(50).max(1);
        limit.min(self.max_action_page_limit)
    }
}

/// Typed failure for the collaboration domain.
#[derive(Debug, Error)]
pub enum CollaborationError {
    #[error("invalid chat {field}: {message}")]
    Invalid {
        field: &'static str,
        message: String,
    },
    #[error("chat body too large: {bytes} bytes (max {max})")]
    BodyTooLarge { bytes: usize, max: usize },
    #[error("chat channel not found: {0}")]
    ChannelNotFound(String),
    #[error("chat message not found: {0}")]
    MessageNotFound(String),
    #[error("stale chat revision for message {message}: expected {expected}, current {current}")]
    RevisionConflict {
        message: String,
        expected: u64,
        current: u64,
    },
    #[error("only the message author may edit or redact chat messages")]
    NotAuthor,
    #[error("chat capacity is exhausted: {0}")]
    Capacity(String),
    #[error("chat store unavailable: {0}")]
    Unavailable(String),
    #[error("chat action not found: {0}")]
    ActionNotFound(String),
    #[error("chat action idempotency conflict for key {0}")]
    ActionConflict(String),
    #[error("chat action denied: {0}")]
    ActionDenied(String),
    #[error("chat project mismatch: {0}")]
    ProjectMismatch(String),
    #[error("chat storage error: {0}")]
    Storage(#[from] StorageError),
}

impl CollaborationError {
    /// Stable wire code for `CoreResponse::Error`.
    pub fn code(&self) -> &'static str {
        match self {
            Self::Invalid { .. } => "chat_invalid_input",
            Self::BodyTooLarge { .. } => "chat_body_too_large",
            Self::ChannelNotFound(_) => "chat_channel_not_found",
            Self::MessageNotFound(_) => "chat_message_not_found",
            Self::RevisionConflict { .. } => "chat_revision_conflict",
            Self::NotAuthor => "chat_not_author",
            Self::Capacity(_) => "chat_capacity",
            Self::Unavailable(_) => "chat_unavailable",
            Self::ActionNotFound(_) => "chat_action_not_found",
            Self::ActionConflict(_) => "chat_action_conflict",
            Self::ActionDenied(_) => "chat_action_denied",
            Self::ProjectMismatch(_) => "chat_project_mismatch",
            Self::Storage(_) => "chat_storage_error",
        }
    }

    pub fn invalid(field: &'static str, message: impl Into<String>) -> Self {
        Self::Invalid {
            field,
            message: message.into(),
        }
    }
}

impl From<sqlx::Error> for CollaborationError {
    fn from(error: sqlx::Error) -> Self {
        Self::Storage(StorageError::Database(error.to_string()))
    }
}

/// Kind of CodeGG object named by a message reference.
///
/// References are opaque locators resolved under the recipient's project
/// authorization at the daemon boundary. Display hints carry no authority
/// and are redacted/truncated before durable write.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChatReferenceKind {
    Session,
    AgentRun,
    Job,
    Commit,
    Artifact,
    Worktree,
    Run,
}

impl ChatReferenceKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Session => "session",
            Self::AgentRun => "agent_run",
            Self::Job => "job",
            Self::Commit => "commit",
            Self::Artifact => "artifact",
            Self::Worktree => "worktree",
            Self::Run => "run",
        }
    }

    /// Parse a wire/storage kind name, failing closed on unknown input.
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "session" => Some(Self::Session),
            "agent_run" => Some(Self::AgentRun),
            "job" => Some(Self::Job),
            "commit" => Some(Self::Commit),
            "artifact" => Some(Self::Artifact),
            "worktree" => Some(Self::Worktree),
            "run" => Some(Self::Run),
            _ => None,
        }
    }

    fn to_dto(self) -> codegg_protocol::core::ChatReferenceKindDto {
        match self {
            Self::Session => codegg_protocol::core::ChatReferenceKindDto::Session,
            Self::AgentRun => codegg_protocol::core::ChatReferenceKindDto::AgentRun,
            Self::Job => codegg_protocol::core::ChatReferenceKindDto::Job,
            Self::Commit => codegg_protocol::core::ChatReferenceKindDto::Commit,
            Self::Artifact => codegg_protocol::core::ChatReferenceKindDto::Artifact,
            Self::Worktree => codegg_protocol::core::ChatReferenceKindDto::Worktree,
            Self::Run => codegg_protocol::core::ChatReferenceKindDto::Run,
        }
    }

    fn from_dto(value: codegg_protocol::core::ChatReferenceKindDto) -> Self {
        match value {
            codegg_protocol::core::ChatReferenceKindDto::Session => Self::Session,
            codegg_protocol::core::ChatReferenceKindDto::AgentRun => Self::AgentRun,
            codegg_protocol::core::ChatReferenceKindDto::Job => Self::Job,
            codegg_protocol::core::ChatReferenceKindDto::Commit => Self::Commit,
            codegg_protocol::core::ChatReferenceKindDto::Artifact => Self::Artifact,
            codegg_protocol::core::ChatReferenceKindDto::Worktree => Self::Worktree,
            codegg_protocol::core::ChatReferenceKindDto::Run => Self::Run,
        }
    }
}

/// One typed reference to a CodeGG object.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChatReference {
    pub kind: ChatReferenceKind,
    pub target_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub display_hint: Option<String>,
}

impl ChatReference {
    fn to_dto(&self) -> codegg_protocol::core::ChatReferenceDto {
        codegg_protocol::core::ChatReferenceDto {
            kind: self.kind.to_dto(),
            target_id: self.target_id.clone(),
            display_hint: self.display_hint.clone(),
        }
    }

    pub(super) fn from_dto(value: &codegg_protocol::core::ChatReferenceDto) -> Self {
        Self {
            kind: ChatReferenceKind::from_dto(value.kind),
            target_id: value.target_id.clone(),
            display_hint: value.display_hint.clone(),
        }
    }
}

/// One durable project channel.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChatChannel {
    pub id: ChannelId,
    pub project_id: ProjectId,
    pub name: String,
    pub created_by: PrincipalId,
    pub created_at_ms: i64,
}

impl ChatChannel {
    pub fn to_dto(&self) -> codegg_protocol::core::ChatChannelDto {
        codegg_protocol::core::ChatChannelDto {
            channel_id: self.id.as_str().to_owned(),
            project_id: self.project_id.as_str().to_owned(),
            name: self.name.clone(),
            created_by: self.created_by.as_str().to_owned(),
            created_at_ms: self.created_at_ms,
        }
    }
}

/// One durable project-chat message.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChatMessage {
    pub id: ChatMessageId,
    pub channel_id: ChannelId,
    pub project_id: ProjectId,
    pub seq: u64,
    pub author_principal: PrincipalId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub author_agent: Option<String>,
    pub body: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reply_to: Option<ChatMessageId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thread_root: Option<ChatMessageId>,
    #[serde(default)]
    pub mentions: Vec<PrincipalId>,
    #[serde(default)]
    pub references: Vec<ChatReference>,
    pub revision: u64,
    pub redacted: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub idempotency_key: Option<String>,
    pub created_at_ms: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub edited_at_ms: Option<i64>,
}

impl ChatMessage {
    pub fn to_dto(&self) -> codegg_protocol::core::ChatMessageDto {
        codegg_protocol::core::ChatMessageDto {
            message_id: self.id.as_str().to_owned(),
            channel_id: self.channel_id.as_str().to_owned(),
            project_id: self.project_id.as_str().to_owned(),
            seq: self.seq,
            author_principal: self.author_principal.as_str().to_owned(),
            author_agent: self.author_agent.clone(),
            body: self.body.clone(),
            reply_to: self.reply_to.as_ref().map(|id| id.as_str().to_owned()),
            thread_root: self.thread_root.as_ref().map(|id| id.as_str().to_owned()),
            mentions: self
                .mentions
                .iter()
                .map(|id| id.as_str().to_owned())
                .collect(),
            references: self.references.iter().map(ChatReference::to_dto).collect(),
            revision: self.revision,
            redacted: self.redacted,
            created_at_ms: self.created_at_ms,
            edited_at_ms: self.edited_at_ms,
        }
    }
}

/// Bounded history page ordered by ascending `seq`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChatHistoryPage {
    pub messages: Vec<ChatMessage>,
    pub next_cursor: u64,
    pub truncated: bool,
    pub retention_floor_seq: u64,
}

/// Bounded incremental sync page with an explicit resync signal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChatSyncPage {
    pub messages: Vec<ChatMessage>,
    pub next_cursor: u64,
    pub resync_required: bool,
    pub retention_floor_seq: u64,
}

/// One durable read marker for a principal in a channel.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChatReadMarker {
    pub channel_id: ChannelId,
    pub principal_id: PrincipalId,
    pub last_read_seq: u64,
    pub updated_at_ms: i64,
}

/// One ephemeral composing entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ComposingEntry {
    pub principal_id: PrincipalId,
    pub client_id: String,
    pub expires_at_ms: i64,
}

impl ComposingEntry {
    pub fn to_dto(&self, channel_id: &ChannelId) -> codegg_protocol::core::ChatComposingDto {
        codegg_protocol::core::ChatComposingDto {
            principal_id: self.principal_id.as_str().to_owned(),
            client_id: self.client_id.clone(),
            expires_at_ms: self.expires_at_ms,
            channel_id: channel_id.as_str().to_owned(),
        }
    }
}

/// Outcome of a send: the durable message plus whether a retry converged
/// on an already-stored row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SendOutcome {
    pub message: ChatMessage,
    pub duplicate: bool,
}

// ── M003: Separately authorized structured chat actions ─────────────────
//
// Free text never executes. A structured action is an explicit typed
// operation naming a message/channel plus a payload and an idempotency
// key. The daemon checks `project.chat` plus the ordinary semantic
// capability for the kind, then calls the existing canonical
// Task/AgentRun/Job services. Chat stores only this
// reference/status projection; the job is canonical durable state
// elsewhere. Retries converge on `(channel_id, idempotency_key)`
// without creating a second job.

/// Kind of structured chat action. Each maps to an existing canonical
/// service and capability:
///
/// - `AgentTask` → `JobKind::Subagent` via `JobSubmissionService`,
///   requires `agent.delegate`.
/// - `ReviewRequest` → `JobKind::Subagent` (reviewer) via
///   `JobSubmissionService`, requires `agent.delegate`.
/// - `JobSubmit` → generic `JobSubmitDto` via `JobSubmissionService`
///   (ToolProgram excluded), requires `job.submit`.
/// - `JobReference` → link an existing job id (no new execution),
///   requires `session.read` to observe the job.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChatActionKind {
    AgentTask,
    ReviewRequest,
    JobSubmit,
    JobReference,
}

impl ChatActionKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::AgentTask => "agent_task",
            Self::ReviewRequest => "review_request",
            Self::JobSubmit => "job_submit",
            Self::JobReference => "job_reference",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "agent_task" => Some(Self::AgentTask),
            "review_request" => Some(Self::ReviewRequest),
            "job_submit" => Some(Self::JobSubmit),
            "job_reference" => Some(Self::JobReference),
            _ => None,
        }
    }

    fn to_dto(self) -> codegg_protocol::core::ChatActionKindDto {
        match self {
            Self::AgentTask => codegg_protocol::core::ChatActionKindDto::AgentTask,
            Self::ReviewRequest => codegg_protocol::core::ChatActionKindDto::ReviewRequest,
            Self::JobSubmit => codegg_protocol::core::ChatActionKindDto::JobSubmit,
            Self::JobReference => codegg_protocol::core::ChatActionKindDto::JobReference,
        }
    }

    /// Ordinary semantic capability required in addition to
    /// `project.chat` for this kind.
    pub const fn required_capability(self) -> &'static str {
        match self {
            Self::AgentTask | Self::ReviewRequest => "agent.delegate",
            Self::JobSubmit => "job.submit",
            Self::JobReference => "session.read",
        }
    }
}

/// One durable structured chat action: reference/status projection only.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChatAction {
    pub id: String,
    pub channel_id: ChannelId,
    pub message_id: crate::identity::ChatMessageId,
    pub project_id: ProjectId,
    pub actor: PrincipalId,
    pub kind: ChatActionKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub job_id: Option<String>,
    pub status: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub idempotency_key: Option<String>,
    pub created_at_ms: i64,
    pub updated_at_ms: i64,
}

impl ChatAction {
    pub fn to_dto(&self) -> codegg_protocol::core::ChatActionDto {
        codegg_protocol::core::ChatActionDto {
            action_id: self.id.clone(),
            channel_id: self.channel_id.as_str().to_owned(),
            message_id: self.message_id.as_str().to_owned(),
            project_id: self.project_id.as_str().to_owned(),
            actor: self.actor.as_str().to_owned(),
            kind: self.kind.to_dto(),
            title: self.title.clone(),
            job_id: self.job_id.clone(),
            status: self.status.clone(),
            created_at_ms: self.created_at_ms,
            updated_at_ms: self.updated_at_ms,
        }
    }
}

/// Outcome of an action submit: the durable projection plus whether a
/// retry converged on an already-stored row (no second job created).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActionOutcome {
    pub action: ChatAction,
    pub duplicate: bool,
}

/// Canonical status strings for [`ChatAction::status`]. Chat never owns
/// execution state: `submitted` means a canonical job was created via
/// the scheduler boundary; `referenced` means an existing job was
/// linked (no new execution). Live job progress is observed through
/// the canonical `JobGet` path, not duplicated here.
pub const CHAT_ACTION_STATUS_SUBMITTED: &str = "submitted";
pub const CHAT_ACTION_STATUS_REFERENCED: &str = "referenced";

// ── Ephemeral composing state ────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct ComposingKey {
    channel_id: String,
    principal_id: String,
    client_id: String,
}

#[derive(Debug, Clone)]
struct ComposingLease {
    expires_at: Instant,
    expires_at_ms: i64,
}

/// Daemon-owned ephemeral composing leases.
///
/// Typing state never reaches durable storage. Restart drops every lease
/// because a fresh service starts empty; expiry drops stale leases on
/// every read through the single [`ComposingState::evict_expired`] path.
#[derive(Debug, Default)]
pub struct ComposingState {
    leases: DashMap<ComposingKey, ComposingLease>,
}

impl ComposingState {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn set(
        &self,
        channel_id: &ChannelId,
        principal: &PrincipalId,
        client_id: &str,
        composing: bool,
        ttl: Duration,
        now: Instant,
        now_ms: i64,
    ) -> Result<(), CollaborationError> {
        if client_id.is_empty() || client_id.len() > 128 {
            return Err(CollaborationError::invalid(
                "client_id",
                "client locator must be 1..=128 bytes",
            ));
        }
        if client_id.bytes().any(|b| b == 0) || client_id.chars().any(char::is_control) {
            return Err(CollaborationError::invalid(
                "client_id",
                "client locator contains an unsupported character",
            ));
        }
        let key = ComposingKey {
            channel_id: channel_id.as_str().to_owned(),
            principal_id: principal.as_str().to_owned(),
            client_id: client_id.to_owned(),
        };
        if composing {
            let expiry_ms = now_ms.saturating_add(ttl.as_millis() as i64);
            self.leases.insert(
                key,
                ComposingLease {
                    expires_at: now + ttl,
                    expires_at_ms: expiry_ms,
                },
            );
        } else {
            self.leases.remove(&key);
        }
        Ok(())
    }

    pub fn evict_expired(&self, now: Instant) -> usize {
        let keys: Vec<ComposingKey> = self
            .leases
            .iter()
            .filter(|entry| now >= entry.value().expires_at)
            .map(|entry| entry.key().clone())
            .collect();
        let removed = keys.len();
        for key in keys {
            self.leases.remove(&key);
        }
        removed
    }

    pub fn list(&self, channel_id: &ChannelId, now: Instant) -> Vec<ComposingEntry> {
        self.evict_expired(now);
        let mut entries: Vec<ComposingEntry> = self
            .leases
            .iter()
            .filter(|entry| entry.key().channel_id == channel_id.as_str())
            .filter_map(|entry| {
                Some(ComposingEntry {
                    principal_id: PrincipalId::parse(&entry.key().principal_id).ok()?,
                    client_id: entry.key().client_id.clone(),
                    expires_at_ms: entry.value().expires_at_ms,
                })
            })
            .collect();
        entries.sort_by(|a, b| {
            a.principal_id
                .as_str()
                .cmp(b.principal_id.as_str())
                .then_with(|| a.client_id.cmp(&b.client_id))
        });
        entries
    }

    pub fn clear(&self) {
        self.leases.clear();
    }

    pub fn live_leases(&self) -> usize {
        self.leases.len()
    }
}

// ── Service ──────────────────────────────────────────────────────────

/// Daemon-owned project-chat service.
///
/// Holds an optional durable pool plus the ephemeral composing map. When
/// no pool is present (legacy in-memory daemons), durable operations
/// fail with [`CollaborationError::Unavailable`] while composing leases
/// still work in memory. The daemon constructs one shared instance so
/// composing leases survive across requests; restart constructs a fresh
/// instance and drops every lease.
#[derive(Debug)]
pub struct CollaborationService {
    pool: Option<SqlitePool>,
    config: CollaborationConfig,
    composing: ComposingState,
}

impl CollaborationService {
    pub fn new(pool: Option<SqlitePool>, config: CollaborationConfig) -> Self {
        Self {
            pool,
            config,
            composing: ComposingState::new(),
        }
    }

    pub fn with_defaults(pool: Option<SqlitePool>) -> Self {
        Self::new(pool, CollaborationConfig::default())
    }

    /// Test seam sharing one pool across cloned handles.
    pub fn with_shared(shared: &Arc<CollaborationService>) -> Self {
        Self::new(shared.pool.clone(), shared.config.clone())
    }

    pub fn config(&self) -> &CollaborationConfig {
        &self.config
    }

    pub fn capabilities_dto(&self) -> codegg_protocol::core::ChatCapabilitiesDto {
        self.config.capabilities_dto()
    }

    fn durable_pool(&self) -> Result<SqlitePool, CollaborationError> {
        self.pool.clone().ok_or_else(|| {
            CollaborationError::Unavailable(
                "project chat requires a durable database pool".to_owned(),
            )
        })
    }

    /// Ensure the default project channel exists (idempotent).
    ///
    /// The first call creates a `general` channel; later calls return the
    /// oldest channel so one default always exists without inventing
    /// multi-channel administration in this milestone.
    pub async fn ensure_default_channel(
        &self,
        project: &ProjectId,
        creator: &PrincipalId,
        now_ms: i64,
    ) -> Result<ChatChannel, CollaborationError> {
        let pool = self.durable_pool()?;
        if let Some(channel) = self.oldest_channel(&pool, project).await? {
            return Ok(channel);
        }
        self.ensure_channel(&pool, project, creator, DEFAULT_CHANNEL_NAME, now_ms)
            .await
    }

    /// Ensure a named project channel exists (idempotent per name).
    ///
    /// Returns the existing channel with `name` when present, otherwise
    /// creates it subject to the per-project channel budget. Names are
    /// matched exactly after trimming; this is the narrow extension
    /// point beyond the default channel, not a full administration API
    /// (no rename/delete in this milestone).
    pub async fn ensure_channel_by_name(
        &self,
        project: &ProjectId,
        creator: &PrincipalId,
        name: &str,
        now_ms: i64,
    ) -> Result<ChatChannel, CollaborationError> {
        let pool = self.durable_pool()?;
        let clean = validate_channel_name(&self.config, name)?;
        if let Some(existing) = self.channel_by_name(&pool, project, &clean).await? {
            return Ok(existing);
        }
        self.ensure_channel(&pool, project, creator, &clean, now_ms)
            .await
    }

    async fn ensure_channel(
        &self,
        pool: &SqlitePool,
        project: &ProjectId,
        creator: &PrincipalId,
        name: &str,
        now_ms: i64,
    ) -> Result<ChatChannel, CollaborationError> {
        if self.channel_count(pool, project).await? >= self.config.max_channels_per_project {
            return Err(CollaborationError::Capacity(
                "project channel budget is exhausted".to_owned(),
            ));
        }
        let id = ChannelId::new();
        sqlx::query(
            "INSERT INTO chat_channel (id, project_id, name, created_by, created_at) \
             VALUES (?, ?, ?, ?, ?)",
        )
        .bind(id.as_str())
        .bind(project.as_str())
        .bind(name)
        .bind(creator.as_str())
        .bind(now_ms)
        .execute(pool)
        .await?;
        Ok(ChatChannel {
            id,
            project_id: project.clone(),
            name: name.to_owned(),
            created_by: creator.clone(),
            created_at_ms: now_ms,
        })
    }

    async fn channel_by_name(
        &self,
        pool: &SqlitePool,
        project: &ProjectId,
        name: &str,
    ) -> Result<Option<ChatChannel>, CollaborationError> {
        let row: Option<(String, String, String, String, i64)> = sqlx::query_as(
            "SELECT id, project_id, name, created_by, created_at FROM chat_channel \
             WHERE project_id = ? AND name = ? ORDER BY created_at ASC, id ASC LIMIT 1",
        )
        .bind(project.as_str())
        .bind(name)
        .fetch_optional(pool)
        .await?;
        row.map(|(id, project_id, name, created_by, created_at)| {
            row_to_channel(id, project_id, name, created_by, created_at)
        })
        .transpose()
    }

    /// Bounded channel listing for one project, oldest first.
    pub async fn list_channels(
        &self,
        project: &ProjectId,
        limit: Option<usize>,
    ) -> Result<(Vec<ChatChannel>, bool), CollaborationError> {
        let pool = self.durable_pool()?;
        let bound = limit
            .unwrap_or(self.config.max_channels_per_project)
            .max(1)
            .min(self.config.max_channels_per_project.max(1));
        let rows: Vec<(String, String, String, String, i64)> = sqlx::query_as(
            "SELECT id, project_id, name, created_by, created_at FROM chat_channel \
             WHERE project_id = ? ORDER BY created_at ASC, id ASC LIMIT ?",
        )
        .bind(project.as_str())
        .bind((bound as i64) + 1)
        .fetch_all(&pool)
        .await?;
        let truncated = rows.len() > bound;
        let mut channels = Vec::with_capacity(rows.len().min(bound));
        for (id, project_id, name, created_by, created_at) in rows.into_iter().take(bound) {
            channels.push(row_to_channel(
                id, project_id, name, created_by, created_at,
            )?);
        }
        Ok((channels, truncated))
    }

    /// Fetch one channel and assert it belongs to `project`.
    async fn channel_in_project(
        &self,
        pool: &SqlitePool,
        project: &ProjectId,
        channel_id: &ChannelId,
    ) -> Result<ChatChannel, CollaborationError> {
        let row: Option<(String, String, String, String, i64)> = sqlx::query_as(
            "SELECT id, project_id, name, created_by, created_at FROM chat_channel WHERE id = ?",
        )
        .bind(channel_id.as_str())
        .fetch_optional(pool)
        .await?;
        let Some((id, stored_project, name, created_by, created_at)) = row else {
            return Err(CollaborationError::ChannelNotFound(
                channel_id.as_str().to_owned(),
            ));
        };
        if stored_project != project.as_str() {
            return Err(CollaborationError::ChannelNotFound(
                channel_id.as_str().to_owned(),
            ));
        }
        row_to_channel(id, stored_project, name, created_by, created_at)
    }

    async fn oldest_channel(
        &self,
        pool: &SqlitePool,
        project: &ProjectId,
    ) -> Result<Option<ChatChannel>, CollaborationError> {
        let row: Option<(String, String, String, String, i64)> = sqlx::query_as(
            "SELECT id, project_id, name, created_by, created_at FROM chat_channel \
             WHERE project_id = ? ORDER BY created_at ASC, id ASC LIMIT 1",
        )
        .bind(project.as_str())
        .fetch_optional(pool)
        .await?;
        row.map(|(id, project_id, name, created_by, created_at)| {
            row_to_channel(id, project_id, name, created_by, created_at)
        })
        .transpose()
    }

    async fn channel_count(
        &self,
        pool: &SqlitePool,
        project: &ProjectId,
    ) -> Result<usize, CollaborationError> {
        let count: (i64,) =
            sqlx::query_as("SELECT COUNT(*) FROM chat_channel WHERE project_id = ?")
                .bind(project.as_str())
                .fetch_one(pool)
                .await?;
        Ok(usize::try_from(count.0).unwrap_or(0))
    }

    /// Send one message. Duplicate idempotency keys return the original.
    #[allow(clippy::too_many_arguments)]
    pub async fn send_message(
        &self,
        project: &ProjectId,
        channel_id: &ChannelId,
        author: &PrincipalId,
        author_agent: Option<&str>,
        body: &str,
        reply_to: Option<&ChatMessageId>,
        thread_root: Option<&ChatMessageId>,
        mentions: &[String],
        references: &[codegg_protocol::core::ChatReferenceDto],
        idempotency_key: Option<&str>,
        now_ms: i64,
    ) -> Result<SendOutcome, CollaborationError> {
        let pool = self.durable_pool()?;
        let channel = self.channel_in_project(&pool, project, channel_id).await?;
        let clean_body = validate_body(&self.config, body)?;
        let agent = author_agent
            .map(|raw| validate_author_agent(&self.config, raw))
            .transpose()?;
        let mention_ids = validate_mentions(&self.config, mentions)?;
        let parsed_references = validate_references(&self.config, references)?;
        let key = idempotency_key
            .map(|raw| validate_idempotency_key(&self.config, raw))
            .transpose()?;

        // Idempotent retransmission: the stored row wins, no new seq.
        if let Some(ref key) = key {
            if let Some(existing) = self.message_by_idempotency(&pool, &channel.id, key).await? {
                return Ok(SendOutcome {
                    message: existing,
                    duplicate: true,
                });
            }
        }

        // Replies and thread roots must name durable messages in the same
        // channel; cross-channel linkage is rejected, never coerced.
        for (field, target) in [("reply_to", reply_to), ("thread_root", thread_root)] {
            if let Some(target) = target {
                let exists: Option<(String,)> =
                    sqlx::query_as("SELECT id FROM chat_message WHERE id = ? AND channel_id = ?")
                        .bind(target.as_str())
                        .bind(channel.id.as_str())
                        .fetch_optional(&pool)
                        .await?;
                if exists.is_none() {
                    return Err(CollaborationError::invalid(
                        field,
                        "reply target must be a message in the same channel",
                    ));
                }
            }
        }

        let mentions_json = encode_mentions(&mention_ids);
        let references_json = encode_references(&parsed_references);
        let message_id = ChatMessageId::new();

        // Deterministic ordering with contention retries: writers racing
        // on MAX(seq)+1 converge via the UNIQUE backstop. The budget
        // covers an N-way burst with head-of-line retries (each retry
        // re-reads MAX, so at most one writer wins per round).
        let mut attempts = 0;
        loop {
            attempts += 1;
            let next_seq: i64 = sqlx::query_scalar(
                "SELECT COALESCE(MAX(seq), 0) + 1 FROM chat_message WHERE channel_id = ?",
            )
            .bind(channel.id.as_str())
            .fetch_one(&pool)
            .await
            .map_err(CollaborationError::from)?;
            let insert = sqlx::query(
                "INSERT INTO chat_message \
                 (id, channel_id, project_id, seq, author_principal, author_agent, body, \
                  reply_to, thread_root, mentions_json, references_json, revision, redacted, \
                  idempotency_key, created_at, edited_at) \
                 VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, 1, 0, ?, ?, NULL)",
            )
            .bind(message_id.as_str())
            .bind(channel.id.as_str())
            .bind(project.as_str())
            .bind(next_seq)
            .bind(author.as_str())
            .bind(agent.clone())
            .bind(&clean_body)
            .bind(reply_to.map(ChatMessageId::as_str))
            .bind(thread_root.map(ChatMessageId::as_str))
            .bind(&mentions_json)
            .bind(&references_json)
            .bind(key.clone())
            .bind(now_ms)
            .execute(&pool)
            .await;
            match insert {
                Ok(_) => {
                    sqlx::query(
                        "INSERT INTO chat_revision \
                         (message_id, revision, body, edited_by, edited_at, reason) \
                         VALUES (?, 1, ?, ?, ?, NULL)",
                    )
                    .bind(message_id.as_str())
                    .bind(&clean_body)
                    .bind(author.as_str())
                    .bind(now_ms)
                    .execute(&pool)
                    .await?;
                    let message = ChatMessage {
                        id: message_id,
                        channel_id: channel.id.clone(),
                        project_id: project.clone(),
                        seq: u64::try_from(next_seq).unwrap_or(0),
                        author_principal: author.clone(),
                        author_agent: agent,
                        body: clean_body,
                        reply_to: reply_to.cloned(),
                        thread_root: thread_root.cloned(),
                        mentions: mention_ids,
                        references: parsed_references,
                        revision: 1,
                        redacted: false,
                        idempotency_key: key,
                        created_at_ms: now_ms,
                        edited_at_ms: None,
                    };
                    self.enforce_retention(&pool, &channel.id).await?;
                    return Ok(SendOutcome {
                        message,
                        duplicate: false,
                    });
                }
                Err(error)
                    if attempts < 10
                        && error
                            .as_database_error()
                            .is_some_and(|db| db.is_unique_violation()) =>
                {
                    // Idempotency races converge on the stored row.
                    if let Some(ref key) = key {
                        if let Some(existing) =
                            self.message_by_idempotency(&pool, &channel.id, key).await?
                        {
                            return Ok(SendOutcome {
                                message: existing,
                                duplicate: true,
                            });
                        }
                    }
                    continue;
                }
                Err(error) => return Err(CollaborationError::from(error)),
            }
        }
    }

    async fn message_by_idempotency(
        &self,
        pool: &SqlitePool,
        channel_id: &ChannelId,
        key: &str,
    ) -> Result<Option<ChatMessage>, CollaborationError> {
        let row: Option<MessageRow> = sqlx::query_as(
            "SELECT id, channel_id, project_id, seq, author_principal, author_agent, body, \
                    reply_to, thread_root, mentions_json, references_json, revision, redacted, \
                    idempotency_key, created_at, edited_at \
             FROM chat_message WHERE channel_id = ? AND idempotency_key = ?",
        )
        .bind(channel_id.as_str())
        .bind(key)
        .fetch_optional(pool)
        .await?;
        row.map(row_to_message).transpose()
    }

    /// Fetch one durable message in channel scope.
    pub async fn get_message(
        &self,
        project: &ProjectId,
        channel_id: &ChannelId,
        message_id: &ChatMessageId,
    ) -> Result<ChatMessage, CollaborationError> {
        let pool = self.durable_pool()?;
        self.channel_in_project(&pool, project, channel_id).await?;
        let row: Option<MessageRow> = sqlx::query_as(
            "SELECT id, channel_id, project_id, seq, author_principal, author_agent, body, \
                    reply_to, thread_root, mentions_json, references_json, revision, redacted, \
                    idempotency_key, created_at, edited_at \
             FROM chat_message WHERE id = ? AND channel_id = ?",
        )
        .bind(message_id.as_str())
        .bind(channel_id.as_str())
        .fetch_optional(&pool)
        .await?;
        let Some(row) = row else {
            return Err(CollaborationError::MessageNotFound(
                message_id.as_str().to_owned(),
            ));
        };
        row_to_message(row)
    }

    /// Edit one message body. Only the original author may edit, and the
    /// expected revision must match the stored revision.
    pub async fn edit_message(
        &self,
        project: &ProjectId,
        channel_id: &ChannelId,
        message_id: &ChatMessageId,
        editor: &PrincipalId,
        expected_revision: u64,
        new_body: &str,
        now_ms: i64,
    ) -> Result<ChatMessage, CollaborationError> {
        let pool = self.durable_pool()?;
        self.channel_in_project(&pool, project, channel_id).await?;
        let clean_body = validate_body(&self.config, new_body)?;
        let current = self.get_message(project, channel_id, message_id).await?;
        if current.author_principal != *editor {
            return Err(CollaborationError::NotAuthor);
        }
        if current.redacted {
            return Err(CollaborationError::invalid(
                "message",
                "redacted messages cannot be edited; history is append-only",
            ));
        }
        if current.revision != expected_revision {
            return Err(CollaborationError::RevisionConflict {
                message: message_id.as_str().to_owned(),
                expected: expected_revision,
                current: current.revision,
            });
        }
        let next_revision = current.revision + 1;
        sqlx::query(
            "UPDATE chat_message SET body = ?, revision = ?, redacted = 0, edited_at = ? \
             WHERE id = ? AND revision = ?",
        )
        .bind(&clean_body)
        .bind(i64::try_from(next_revision).unwrap_or(i64::MAX))
        .bind(now_ms)
        .bind(message_id.as_str())
        .bind(i64::try_from(current.revision).unwrap_or(i64::MAX))
        .execute(&pool)
        .await?;
        sqlx::query(
            "INSERT INTO chat_revision (message_id, revision, body, edited_by, edited_at, reason) \
             VALUES (?, ?, ?, ?, ?, NULL)",
        )
        .bind(message_id.as_str())
        .bind(i64::try_from(next_revision).unwrap_or(i64::MAX))
        .bind(&clean_body)
        .bind(editor.as_str())
        .bind(now_ms)
        .execute(&pool)
        .await?;
        let mut updated = current;
        updated.body = clean_body;
        updated.revision = next_revision;
        updated.edited_at_ms = Some(now_ms);
        Ok(updated)
    }

    /// Redact one message body. Only the original author may redact; the
    /// stored body becomes [`REDACTED_BODY`] while the prior revisions
    /// remain in `chat_revision` as append-only history.
    pub async fn redact_message(
        &self,
        project: &ProjectId,
        channel_id: &ChannelId,
        message_id: &ChatMessageId,
        editor: &PrincipalId,
        expected_revision: Option<u64>,
        reason: Option<&str>,
        now_ms: i64,
    ) -> Result<ChatMessage, CollaborationError> {
        let pool = self.durable_pool()?;
        self.channel_in_project(&pool, project, channel_id).await?;
        let current = self.get_message(project, channel_id, message_id).await?;
        if current.author_principal != *editor {
            return Err(CollaborationError::NotAuthor);
        }
        if let Some(expected) = expected_revision {
            if current.revision != expected {
                return Err(CollaborationError::RevisionConflict {
                    message: message_id.as_str().to_owned(),
                    expected,
                    current: current.revision,
                });
            }
        }
        let clean_reason = reason
            .map(str::trim)
            .filter(|r| !r.is_empty())
            .map(|r| r.chars().take(200).collect::<String>());
        if let Some(ref r) = clean_reason {
            if r.bytes().any(|b| b == 0) || r.chars().any(char::is_control) {
                return Err(CollaborationError::invalid(
                    "reason",
                    "redaction reason contains an unsupported character",
                ));
            }
            let (_, secret) = redact_secrets_in_body(r);
            if secret {
                return Err(CollaborationError::invalid(
                    "reason",
                    "redaction reason must not carry credential-like material",
                ));
            }
        }
        let next_revision = current.revision + 1;
        sqlx::query(
            "UPDATE chat_message SET body = ?, revision = ?, redacted = 1, edited_at = ? \
             WHERE id = ?",
        )
        .bind(REDACTED_BODY)
        .bind(i64::try_from(next_revision).unwrap_or(i64::MAX))
        .bind(now_ms)
        .bind(message_id.as_str())
        .execute(&pool)
        .await?;
        sqlx::query(
            "INSERT INTO chat_revision (message_id, revision, body, edited_by, edited_at, reason) \
             VALUES (?, ?, ?, ?, ?, ?)",
        )
        .bind(message_id.as_str())
        .bind(i64::try_from(next_revision).unwrap_or(i64::MAX))
        .bind(REDACTED_BODY)
        .bind(editor.as_str())
        .bind(now_ms)
        .bind(clean_reason)
        .execute(&pool)
        .await?;
        let mut updated = current;
        updated.body = REDACTED_BODY.to_owned();
        updated.revision = next_revision;
        updated.redacted = true;
        updated.edited_at_ms = Some(now_ms);
        Ok(updated)
    }

    /// Bounded history page for one channel, ascending by `seq`.
    pub async fn history(
        &self,
        project: &ProjectId,
        channel_id: &ChannelId,
        from_seq: Option<u64>,
        limit: Option<u32>,
    ) -> Result<ChatHistoryPage, CollaborationError> {
        let pool = self.durable_pool()?;
        self.channel_in_project(&pool, project, channel_id).await?;
        let bound = self.config.clamp_page_limit(limit);
        let from = from_seq.unwrap_or(0);
        let floor = self.retention_floor(&pool, channel_id).await?;
        let rows = self.page_rows(&pool, channel_id, from, bound).await?;
        let truncated = rows.len() > bound as usize;
        let mut messages = Vec::with_capacity(rows.len().min(bound as usize));
        for row in rows.into_iter().take(bound as usize) {
            messages.push(row);
        }
        let next_cursor = messages.last().map(|m| m.seq + 1).unwrap_or(from);
        Ok(ChatHistoryPage {
            messages,
            next_cursor,
            truncated,
            retention_floor_seq: floor,
        })
    }

    /// Bounded incremental sync from a cursor.
    ///
    /// Cursors at or above the retention floor resume incrementally.
    /// Expired cursors (`from_seq < floor`) return `resync_required` with
    /// the oldest retained page so reconnects converge without replaying
    /// pruned history.
    pub async fn sync(
        &self,
        project: &ProjectId,
        channel_id: &ChannelId,
        from_seq: u64,
        limit: Option<u32>,
    ) -> Result<ChatSyncPage, CollaborationError> {
        let pool = self.durable_pool()?;
        self.channel_in_project(&pool, project, channel_id).await?;
        let bound = self.config.clamp_page_limit(limit);
        let floor = self.retention_floor(&pool, channel_id).await?;
        if from_seq < floor {
            let rows = self.page_rows(&pool, channel_id, floor, bound).await?;
            let mut messages = Vec::with_capacity(rows.len().min(bound as usize));
            for row in rows.into_iter().take(bound as usize) {
                messages.push(row);
            }
            let next_cursor = messages.last().map(|m| m.seq + 1).unwrap_or(floor);
            return Ok(ChatSyncPage {
                messages,
                next_cursor,
                resync_required: true,
                retention_floor_seq: floor,
            });
        }
        let rows = self.page_rows(&pool, channel_id, from_seq, bound).await?;
        let mut messages = Vec::with_capacity(rows.len().min(bound as usize));
        for row in rows.into_iter().take(bound as usize) {
            messages.push(row);
        }
        let next_cursor = messages.last().map(|m| m.seq + 1).unwrap_or(from_seq);
        Ok(ChatSyncPage {
            messages,
            next_cursor,
            resync_required: false,
            retention_floor_seq: floor,
        })
    }

    async fn page_rows(
        &self,
        pool: &SqlitePool,
        channel_id: &ChannelId,
        from_seq: u64,
        bound: u32,
    ) -> Result<Vec<ChatMessage>, CollaborationError> {
        let rows: Vec<MessageRow> = sqlx::query_as(
            "SELECT id, channel_id, project_id, seq, author_principal, author_agent, body, \
                    reply_to, thread_root, mentions_json, references_json, revision, redacted, \
                    idempotency_key, created_at, edited_at \
             FROM chat_message WHERE channel_id = ? AND seq >= ? \
             ORDER BY seq ASC LIMIT ?",
        )
        .bind(channel_id.as_str())
        .bind(i64::try_from(from_seq).unwrap_or(i64::MAX))
        .bind(i64::from(bound) + 1)
        .fetch_all(pool)
        .await?;
        let mut messages = Vec::with_capacity(rows.len());
        for row in rows {
            messages.push(row_to_message(row)?);
        }
        Ok(messages)
    }

    async fn retention_floor(
        &self,
        pool: &SqlitePool,
        channel_id: &ChannelId,
    ) -> Result<u64, CollaborationError> {
        let row: Option<(Option<i64>,)> =
            sqlx::query_as("SELECT MIN(seq) FROM chat_message WHERE channel_id = ?")
                .bind(channel_id.as_str())
                .fetch_optional(pool)
                .await?;
        Ok(row
            .and_then(|(min,)| min)
            .map(|min| u64::try_from(min).unwrap_or(0))
            .unwrap_or(0))
    }

    /// Enforce the per-channel retention window, deleting oldest first.
    /// Returns the number of pruned messages.
    pub async fn enforce_retention(
        &self,
        pool: &SqlitePool,
        channel_id: &ChannelId,
    ) -> Result<usize, CollaborationError> {
        let count: (i64,) =
            sqlx::query_as("SELECT COUNT(*) FROM chat_message WHERE channel_id = ?")
                .bind(channel_id.as_str())
                .fetch_one(pool)
                .await?;
        let total = usize::try_from(count.0).unwrap_or(0);
        if total <= self.config.max_messages_per_channel {
            return Ok(0);
        }
        let overflow = total - self.config.max_messages_per_channel;
        let victims: Vec<(String,)> = sqlx::query_as(
            "SELECT id FROM chat_message WHERE channel_id = ? ORDER BY seq ASC LIMIT ?",
        )
        .bind(channel_id.as_str())
        .bind(overflow as i64)
        .fetch_all(pool)
        .await?;
        // Revision rows survive pruning so the append-only history is
        // preserved even after the live window rolls forward.
        for (id,) in &victims {
            sqlx::query("DELETE FROM chat_message WHERE id = ?")
                .bind(id)
                .execute(pool)
                .await?;
        }
        Ok(victims.len())
    }

    /// Explicit retention prune used by tests and maintenance ticks.
    pub async fn prune_retention(
        &self,
        project: &ProjectId,
        channel_id: &ChannelId,
    ) -> Result<usize, CollaborationError> {
        let pool = self.durable_pool()?;
        self.channel_in_project(&pool, project, channel_id).await?;
        self.enforce_retention(&pool, channel_id).await
    }

    /// Count of durable messages in one channel.
    pub async fn message_count(
        &self,
        project: &ProjectId,
        channel_id: &ChannelId,
    ) -> Result<usize, CollaborationError> {
        let pool = self.durable_pool()?;
        self.channel_in_project(&pool, project, channel_id).await?;
        let count: (i64,) =
            sqlx::query_as("SELECT COUNT(*) FROM chat_message WHERE channel_id = ?")
                .bind(channel_id.as_str())
                .fetch_one(&pool)
                .await?;
        Ok(usize::try_from(count.0).unwrap_or(0))
    }

    /// Count of preserved revision rows for one message (audit-history proof).
    pub async fn revision_count(
        &self,
        message_id: &ChatMessageId,
    ) -> Result<usize, CollaborationError> {
        let pool = self.durable_pool()?;
        let count: (i64,) =
            sqlx::query_as("SELECT COUNT(*) FROM chat_revision WHERE message_id = ?")
                .bind(message_id.as_str())
                .fetch_one(&pool)
                .await?;
        Ok(usize::try_from(count.0).unwrap_or(0))
    }

    /// Advance the caller's read marker. Markers only move forward; a
    /// stale `last_read_seq` leaves the stored marker untouched.
    pub async fn set_read_marker(
        &self,
        project: &ProjectId,
        channel_id: &ChannelId,
        principal: &PrincipalId,
        last_read_seq: u64,
        now_ms: i64,
    ) -> Result<ChatReadMarker, CollaborationError> {
        let pool = self.durable_pool()?;
        self.channel_in_project(&pool, project, channel_id).await?;
        let current = self.get_read_marker(project, channel_id, principal).await?;
        if let Some(ref marker) = current {
            if last_read_seq <= marker.last_read_seq {
                return Ok(marker.clone());
            }
        }
        sqlx::query(
            "INSERT INTO chat_read_marker (channel_id, principal_id, last_read_seq, updated_at) \
             VALUES (?, ?, ?, ?) \
             ON CONFLICT(channel_id, principal_id) DO UPDATE SET \
               last_read_seq = excluded.last_read_seq, updated_at = excluded.updated_at",
        )
        .bind(channel_id.as_str())
        .bind(principal.as_str())
        .bind(i64::try_from(last_read_seq).unwrap_or(i64::MAX))
        .bind(now_ms)
        .execute(&pool)
        .await?;
        Ok(ChatReadMarker {
            channel_id: channel_id.clone(),
            principal_id: principal.clone(),
            last_read_seq,
            updated_at_ms: now_ms,
        })
    }

    /// Fetch the caller's read marker, if any.
    pub async fn get_read_marker(
        &self,
        project: &ProjectId,
        channel_id: &ChannelId,
        principal: &PrincipalId,
    ) -> Result<Option<ChatReadMarker>, CollaborationError> {
        let pool = self.durable_pool()?;
        self.channel_in_project(&pool, project, channel_id).await?;
        let row: Option<(i64, i64)> = sqlx::query_as(
            "SELECT last_read_seq, updated_at FROM chat_read_marker \
             WHERE channel_id = ? AND principal_id = ?",
        )
        .bind(channel_id.as_str())
        .bind(principal.as_str())
        .fetch_optional(&pool)
        .await?;
        Ok(row.map(|(seq, updated)| ChatReadMarker {
            channel_id: channel_id.clone(),
            principal_id: principal.clone(),
            last_read_seq: u64::try_from(seq).unwrap_or(0),
            updated_at_ms: updated,
        }))
    }

    // ── Ephemeral composing ──────────────────────────────────────────

    /// Set or clear the caller's composing lease in one channel.
    pub fn set_composing(
        &self,
        channel_id: &ChannelId,
        principal: &PrincipalId,
        client_id: &str,
        composing: bool,
        now: Instant,
        now_ms: i64,
    ) -> Result<(), CollaborationError> {
        self.composing.set(
            channel_id,
            principal,
            client_id,
            composing,
            self.config.composing_ttl,
            now,
            now_ms,
        )
    }

    /// Bounded composing snapshot for one channel (expired leases dropped).
    pub fn list_composing(&self, channel_id: &ChannelId, now: Instant) -> Vec<ComposingEntry> {
        self.composing.list(channel_id, now)
    }

    /// Drop every composing lease (daemon restart). Rebuilds exclusively
    /// from active heartbeats afterwards; restart never fabricates state.
    pub fn clear_composing(&self) {
        self.composing.clear();
    }

    // ── M003: structured chat actions ────────────────────────────────

    fn row_to_action(row: ActionRow) -> Result<ChatAction, CollaborationError> {
        let (
            action_id,
            channel_id,
            message_id,
            project_id,
            actor,
            kind_raw,
            title,
            job_id,
            status,
            idempotency_key,
            created_at,
            updated_at,
        ) = row;
        let bad = |field: &'static str| {
            CollaborationError::invalid(field, "stored action row failed to parse")
        };
        let kind = ChatActionKind::parse(&kind_raw).ok_or(bad("action_kind"))?;
        if status != CHAT_ACTION_STATUS_SUBMITTED && status != CHAT_ACTION_STATUS_REFERENCED {
            return Err(bad("action_status"));
        }
        Ok(ChatAction {
            id: action_id,
            channel_id: ChannelId::parse(&channel_id).map_err(|_| bad("channel_id"))?,
            message_id: crate::identity::ChatMessageId::parse(&message_id)
                .map_err(|_| bad("message_id"))?,
            project_id: ProjectId::parse(&project_id).map_err(|_| bad("project_id"))?,
            actor: PrincipalId::parse(&actor).map_err(|_| bad("actor"))?,
            kind,
            title,
            job_id,
            status,
            idempotency_key: Some(idempotency_key),
            created_at_ms: created_at,
            updated_at_ms: updated_at,
        })
    }

    /// Fetch one action by channel-scoped idempotency key, if present.
    /// Used for retry convergence: a duplicate key returns the stored
    /// projection without creating a second job.
    pub async fn find_action_by_idempotency(
        &self,
        project: &ProjectId,
        channel_id: &ChannelId,
        idempotency_key: &str,
    ) -> Result<Option<ChatAction>, CollaborationError> {
        let pool = self.durable_pool()?;
        self.channel_in_project(&pool, project, channel_id).await?;
        let row: Option<ActionRow> = sqlx::query_as(
            "SELECT action_id, channel_id, message_id, project_id, actor, kind, title, \
                    job_id, status, idempotency_key, created_at, updated_at \
             FROM chat_action WHERE channel_id = ? AND idempotency_key = ?",
        )
        .bind(channel_id.as_str())
        .bind(idempotency_key)
        .fetch_optional(&pool)
        .await?;
        row.map(Self::row_to_action).transpose()
    }

    /// Insert one action projection. The `(channel_id, idempotency_key)`
    /// unique index is the retry backstop: concurrent inserts with the
    /// same key converge via the stored row (callers map the unique
    /// violation to a re-read, never to a second job).
    #[allow(clippy::too_many_arguments)]
    pub async fn insert_action(
        &self,
        project: &ProjectId,
        channel_id: &ChannelId,
        message_id: &crate::identity::ChatMessageId,
        actor: &PrincipalId,
        kind: ChatActionKind,
        title: Option<String>,
        job_id: Option<String>,
        status: &str,
        idempotency_key: &str,
        now_ms: i64,
    ) -> Result<ChatAction, CollaborationError> {
        let pool = self.durable_pool()?;
        self.channel_in_project(&pool, project, channel_id).await?;
        // The linked message must exist in the same channel; cross-channel
        // linkage is rejected, never coerced. This also enforces the
        // message/project match: the channel row already binds the
        // project, so a message from another project cannot be named here.
        let _linked = self
            .get_message(project, channel_id, message_id)
            .await
            .map_err(|_| {
                CollaborationError::ProjectMismatch(
                    "action message does not belong to the named channel/project".to_owned(),
                )
            })?;
        let clean_title = validate_action_title(&self.config, title.as_deref())?;
        let clean_key = validate_idempotency_key(&self.config, idempotency_key)?;
        if status != CHAT_ACTION_STATUS_SUBMITTED && status != CHAT_ACTION_STATUS_REFERENCED {
            return Err(CollaborationError::invalid(
                "action_status",
                "action status must be submitted or referenced",
            ));
        }
        let action_id = uuid::Uuid::new_v4().to_string();
        let insert = sqlx::query(
            "INSERT INTO chat_action \
             (action_id, channel_id, message_id, project_id, actor, kind, title, \
              job_id, status, idempotency_key, created_at, updated_at) \
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(&action_id)
        .bind(channel_id.as_str())
        .bind(message_id.as_str())
        .bind(project.as_str())
        .bind(actor.as_str())
        .bind(kind.as_str())
        .bind(clean_title.clone())
        .bind(job_id.clone())
        .bind(status)
        .bind(&clean_key)
        .bind(now_ms)
        .bind(now_ms)
        .execute(&pool)
        .await;
        match insert {
            Ok(_) => Ok(ChatAction {
                id: action_id,
                channel_id: channel_id.clone(),
                message_id: message_id.clone(),
                project_id: project.clone(),
                actor: actor.clone(),
                kind,
                title: clean_title,
                job_id,
                status: status.to_owned(),
                idempotency_key: Some(clean_key),
                created_at_ms: now_ms,
                updated_at_ms: now_ms,
            }),
            Err(error)
                if error
                    .as_database_error()
                    .is_some_and(|db| db.is_unique_violation()) =>
            {
                Err(CollaborationError::ActionConflict(clean_key))
            }
            Err(error) => Err(CollaborationError::from(error)),
        }
    }

    /// Fetch one action in channel scope, asserting channel/project binding.
    pub async fn get_action(
        &self,
        project: &ProjectId,
        channel_id: &ChannelId,
        action_id: &str,
    ) -> Result<ChatAction, CollaborationError> {
        let pool = self.durable_pool()?;
        self.channel_in_project(&pool, project, channel_id).await?;
        if action_id.is_empty() || action_id.len() > 128 {
            return Err(CollaborationError::invalid(
                "action_id",
                "action id must be 1..=128 bytes",
            ));
        }
        let row: Option<ActionRow> = sqlx::query_as(
            "SELECT action_id, channel_id, message_id, project_id, actor, kind, title, \
                    job_id, status, idempotency_key, created_at, updated_at \
             FROM chat_action WHERE action_id = ? AND channel_id = ?",
        )
        .bind(action_id)
        .bind(channel_id.as_str())
        .fetch_optional(&pool)
        .await?;
        let Some(row) = row else {
            return Err(CollaborationError::ActionNotFound(action_id.to_owned()));
        };
        let action = Self::row_to_action(row)?;
        if action.project_id != *project {
            return Err(CollaborationError::ActionNotFound(action_id.to_owned()));
        }
        Ok(action)
    }

    /// Bounded action listing for one channel, oldest first, optionally
    /// filtered to one message.
    pub async fn list_actions(
        &self,
        project: &ProjectId,
        channel_id: &ChannelId,
        message_id: Option<&crate::identity::ChatMessageId>,
        limit: Option<u32>,
    ) -> Result<Vec<ChatAction>, CollaborationError> {
        let pool = self.durable_pool()?;
        self.channel_in_project(&pool, project, channel_id).await?;
        let bound = self.config.clamp_action_page_limit(limit);
        let rows: Vec<ActionRow> = if let Some(message) = message_id {
            sqlx::query_as(
                "SELECT action_id, channel_id, message_id, project_id, actor, kind, title, \
                        job_id, status, idempotency_key, created_at, updated_at \
                 FROM chat_action WHERE channel_id = ? AND message_id = ? \
                 ORDER BY created_at ASC, action_id ASC LIMIT ?",
            )
            .bind(channel_id.as_str())
            .bind(message.as_str())
            .bind(i64::from(bound) + 1)
            .fetch_all(&pool)
            .await?
        } else {
            sqlx::query_as(
                "SELECT action_id, channel_id, message_id, project_id, actor, kind, title, \
                        job_id, status, idempotency_key, created_at, updated_at \
                 FROM chat_action WHERE channel_id = ? \
                 ORDER BY created_at ASC, action_id ASC LIMIT ?",
            )
            .bind(channel_id.as_str())
            .bind(i64::from(bound) + 1)
            .fetch_all(&pool)
            .await?
        };
        let mut actions = Vec::with_capacity(rows.len().min(bound as usize));
        for row in rows.into_iter().take(bound as usize) {
            let action = Self::row_to_action(row)?;
            if action.project_id != *project {
                continue;
            }
            actions.push(action);
        }
        Ok(actions)
    }

    /// Count of durable actions in one channel (test/observability seam).
    pub async fn action_count(
        &self,
        project: &ProjectId,
        channel_id: &ChannelId,
    ) -> Result<usize, CollaborationError> {
        let pool = self.durable_pool()?;
        self.channel_in_project(&pool, project, channel_id).await?;
        let count: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM chat_action WHERE channel_id = ?")
            .bind(channel_id.as_str())
            .fetch_one(&pool)
            .await?;
        Ok(usize::try_from(count.0).unwrap_or(0))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn test_pool() -> SqlitePool {
        let pool = SqlitePool::connect("sqlite::memory:")
            .await
            .expect("in-memory pool");
        crate::session::schema::migrate(&pool)
            .await
            .expect("migrate test pool");
        pool
    }

    fn service_with_pool(pool: SqlitePool) -> CollaborationService {
        CollaborationService::new(Some(pool), CollaborationConfig::default())
    }

    fn small_retention_service(pool: SqlitePool) -> CollaborationService {
        CollaborationService::new(
            Some(pool),
            CollaborationConfig {
                max_messages_per_channel: 5,
                ..CollaborationConfig::default()
            },
        )
    }

    fn ids() -> (ProjectId, PrincipalId, PrincipalId) {
        (
            ProjectId::parse("project-1").unwrap(),
            PrincipalId::parse("principal-author").unwrap(),
            PrincipalId::parse("principal-other").unwrap(),
        )
    }

    #[test]
    fn secret_redaction_replaces_values_not_messages() {
        let (body, redacted) = redact_secrets_in_body("hello api_key=supersecret123 world");
        assert!(redacted);
        assert!(body.contains(REDACTED_MARKER));
        assert!(!body.contains("supersecret123"));

        let (body, redacted) = redact_secrets_in_body("token: abcdefghijklmnop rest");
        assert!(redacted);
        assert!(body.contains(REDACTED_MARKER));

        let (body, redacted) = redact_secrets_in_body("deploy AKIAIOSFODNN7EXAMPLE now");
        assert!(redacted);
        assert!(!body.contains("AKIAIOSFODNN7EXAMPLE"));

        let (body, redacted) = redact_secrets_in_body("just a normal project update");
        assert!(!redacted);
        assert_eq!(body, "just a normal project update");
    }

    #[test]
    fn reference_kinds_round_trip_closed() {
        for kind in [
            ChatReferenceKind::Session,
            ChatReferenceKind::AgentRun,
            ChatReferenceKind::Job,
            ChatReferenceKind::Commit,
            ChatReferenceKind::Artifact,
            ChatReferenceKind::Worktree,
            ChatReferenceKind::Run,
        ] {
            assert_eq!(ChatReferenceKind::parse(kind.as_str()), Some(kind));
        }
        assert_eq!(ChatReferenceKind::parse("execute"), None);
        assert_eq!(ChatReferenceKind::parse(""), None);
    }

    #[test]
    fn body_bounds_reject_empty_oversized_and_nul() {
        let config = CollaborationConfig::default();
        assert!(validate_body(&config, "").is_err());
        assert!(validate_body(&config, "ok").is_ok());
        let oversized = "x".repeat(config.max_body_bytes + 1);
        assert!(matches!(
            validate_body(&config, &oversized),
            Err(CollaborationError::BodyTooLarge { .. })
        ));
        assert!(validate_body(&config, "a\0b").is_err());
        // Newlines and tabs are legitimate chat content.
        assert!(validate_body(&config, "line one\nline two\ttab").is_ok());
    }

    #[test]
    fn audit_metadata_carries_no_body_material() {
        let (project, author, _) = ids();
        let message = ChatMessage {
            id: ChatMessageId::parse("chat-message-1").unwrap(),
            channel_id: ChannelId::parse("channel-1").unwrap(),
            project_id: project,
            seq: 7,
            author_principal: author,
            author_agent: None,
            body: "api_key=hunter2 secret payload".to_owned(),
            reply_to: None,
            thread_root: None,
            mentions: Vec::new(),
            references: Vec::new(),
            revision: 2,
            redacted: false,
            idempotency_key: None,
            created_at_ms: 1_000,
            edited_at_ms: None,
        };
        let metadata = audit_metadata_for_message(&message);
        let joined = metadata.values().cloned().collect::<Vec<_>>().join(" ");
        assert!(!joined.contains("hunter2"));
        assert!(!joined.contains("secret payload"));
        assert_eq!(metadata.get("chat.seq").map(String::as_str), Some("7"));
        assert_eq!(metadata.get("chat.revision").map(String::as_str), Some("2"));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn default_channel_is_idempotent_per_project() {
        let pool = test_pool().await;
        let service = service_with_pool(pool);
        let (project, author, _) = ids();
        let first = service
            .ensure_default_channel(&project, &author, 1_000)
            .await
            .unwrap();
        let second = service
            .ensure_default_channel(&project, &author, 2_000)
            .await
            .unwrap();
        assert_eq!(first.id, second.id);
        assert_eq!(first.name, DEFAULT_CHANNEL_NAME);
        let (channels, truncated) = service.list_channels(&project, None).await.unwrap();
        assert_eq!(channels.len(), 1);
        assert!(!truncated);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn named_channel_ensure_is_idempotent_per_name() {
        let pool = test_pool().await;
        let service = service_with_pool(pool);
        let (project, author, _) = ids();
        let first = service
            .ensure_channel_by_name(&project, &author, "design", 1_000)
            .await
            .unwrap();
        assert_eq!(first.name, "design");
        let second = service
            .ensure_channel_by_name(&project, &author, "design", 2_000)
            .await
            .unwrap();
        assert_eq!(first.id, second.id);
        // A distinct name creates a distinct channel.
        let other = service
            .ensure_channel_by_name(&project, &author, "ops", 3_000)
            .await
            .unwrap();
        assert_ne!(first.id, other.id);
        // The default ensure still converges on the oldest channel.
        let default = service
            .ensure_default_channel(&project, &author, 4_000)
            .await
            .unwrap();
        assert_eq!(default.id, first.id);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn send_assigns_deterministic_order_and_idempotency_converges() {
        let pool = test_pool().await;
        let service = service_with_pool(pool);
        let (project, author, _) = ids();
        let channel = service
            .ensure_default_channel(&project, &author, 1_000)
            .await
            .unwrap();
        let first = service
            .send_message(
                &project,
                &channel.id,
                &author,
                None,
                "first",
                None,
                None,
                &[],
                &[],
                Some("key-1"),
                1_001,
            )
            .await
            .unwrap();
        assert!(!first.duplicate);
        assert_eq!(first.message.seq, 1);
        let retry = service
            .send_message(
                &project,
                &channel.id,
                &author,
                None,
                "first-changed-body",
                None,
                None,
                &[],
                &[],
                Some("key-1"),
                1_002,
            )
            .await
            .unwrap();
        assert!(retry.duplicate);
        assert_eq!(retry.message.id, first.message.id);
        assert_eq!(retry.message.seq, 1);
        assert_eq!(retry.message.body, "first");
        let second = service
            .send_message(
                &project,
                &channel.id,
                &author,
                None,
                "second",
                None,
                None,
                &[],
                &[],
                Some("key-2"),
                1_003,
            )
            .await
            .unwrap();
        assert_eq!(second.message.seq, 2);
        assert_eq!(
            service.message_count(&project, &channel.id).await.unwrap(),
            2
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn reply_thread_mention_reference_round_trip() {
        let pool = test_pool().await;
        let service = service_with_pool(pool);
        let (project, author, other) = ids();
        let channel = service
            .ensure_default_channel(&project, &author, 1_000)
            .await
            .unwrap();
        let parent = service
            .send_message(
                &project,
                &channel.id,
                &author,
                Some("agent-main"),
                "parent message",
                None,
                None,
                &[],
                &[],
                None,
                1_001,
            )
            .await
            .unwrap()
            .message;
        let reply = service
            .send_message(
                &project,
                &channel.id,
                &other,
                None,
                "reply with mention",
                Some(&parent.id),
                Some(&parent.id),
                &[other.as_str().to_owned()],
                &[codegg_protocol::core::ChatReferenceDto {
                    kind: codegg_protocol::core::ChatReferenceKindDto::Session,
                    target_id: "session-1".to_owned(),
                    display_hint: Some("design session".to_owned()),
                }],
                None,
                1_002,
            )
            .await
            .unwrap()
            .message;
        assert_eq!(reply.reply_to, Some(parent.id.clone()));
        assert_eq!(reply.thread_root, Some(parent.id.clone()));
        assert_eq!(reply.mentions, vec![other.clone()]);
        assert_eq!(reply.references.len(), 1);
        assert_eq!(reply.references[0].kind, ChatReferenceKind::Session);
        // Cross-channel reply linkage is rejected, never coerced.
        let outsider_project = ProjectId::parse("project-2").unwrap();
        let other_channel = service
            .ensure_default_channel(&outsider_project, &other, 1_003)
            .await
            .unwrap();
        let bad = service
            .send_message(
                &outsider_project,
                &other_channel.id,
                &other,
                None,
                "bad reply",
                Some(&parent.id),
                None,
                &[],
                &[],
                None,
                1_004,
            )
            .await;
        assert!(matches!(bad, Err(CollaborationError::Invalid { .. })));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn edit_redact_revision_conflicts_and_history() {
        let pool = test_pool().await;
        let service = service_with_pool(pool);
        let (project, author, other) = ids();
        let channel = service
            .ensure_default_channel(&project, &author, 1_000)
            .await
            .unwrap();
        let sent = service
            .send_message(
                &project,
                &channel.id,
                &author,
                None,
                "original",
                None,
                None,
                &[],
                &[],
                None,
                1_001,
            )
            .await
            .unwrap()
            .message;
        // Non-authors cannot edit.
        assert!(matches!(
            service
                .edit_message(&project, &channel.id, &sent.id, &other, 1, "hijack", 1_002)
                .await,
            Err(CollaborationError::NotAuthor)
        ));
        // Stale revisions conflict with a typed error.
        assert!(matches!(
            service
                .edit_message(&project, &channel.id, &sent.id, &author, 99, "stale", 1_002)
                .await,
            Err(CollaborationError::RevisionConflict { .. })
        ));
        let edited = service
            .edit_message(&project, &channel.id, &sent.id, &author, 1, "edited", 1_003)
            .await
            .unwrap();
        assert_eq!(edited.revision, 2);
        assert_eq!(edited.body, "edited");
        let redacted = service
            .redact_message(
                &project,
                &channel.id,
                &sent.id,
                &author,
                Some(2),
                Some("cleanup"),
                1_004,
            )
            .await
            .unwrap();
        assert!(redacted.redacted);
        assert_eq!(redacted.body, REDACTED_BODY);
        assert_eq!(redacted.revision, 3);
        // Append-only history preserves every revision.
        assert_eq!(service.revision_count(&sent.id).await.unwrap(), 3);
        // Redacted messages refuse further edits.
        assert!(service
            .edit_message(&project, &channel.id, &sent.id, &author, 3, "again", 1_005)
            .await
            .is_err());
    }

    #[tokio::test(flavor = "current_thread")]
    async fn retention_prunes_oldest_and_sync_resyncs() {
        let pool = test_pool().await;
        let service = small_retention_service(pool);
        let (project, author, _) = ids();
        let channel = service
            .ensure_default_channel(&project, &author, 1_000)
            .await
            .unwrap();
        for i in 0..8 {
            service
                .send_message(
                    &project,
                    &channel.id,
                    &author,
                    None,
                    &format!("message-{i}"),
                    None,
                    None,
                    &[],
                    &[],
                    None,
                    1_001 + i,
                )
                .await
                .unwrap();
        }
        assert_eq!(
            service.message_count(&project, &channel.id).await.unwrap(),
            5
        );
        // Expired cursor resyncs to the oldest retained page.
        let resync = service
            .sync(&project, &channel.id, 1, Some(50))
            .await
            .unwrap();
        assert!(resync.resync_required);
        assert_eq!(resync.retention_floor_seq, 4);
        assert!(!resync.messages.is_empty());
        // Fresh cursor resumes incrementally.
        let live = service
            .sync(&project, &channel.id, resync.next_cursor, Some(50))
            .await
            .unwrap();
        assert!(!live.resync_required);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn read_markers_move_forward_only() {
        let pool = test_pool().await;
        let service = service_with_pool(pool);
        let (project, author, reader) = ids();
        let channel = service
            .ensure_default_channel(&project, &author, 1_000)
            .await
            .unwrap();
        for i in 0..3 {
            service
                .send_message(
                    &project,
                    &channel.id,
                    &author,
                    None,
                    &format!("m{i}"),
                    None,
                    None,
                    &[],
                    &[],
                    None,
                    1_001 + i,
                )
                .await
                .unwrap();
        }
        let marker = service
            .set_read_marker(&project, &channel.id, &reader, 2, 2_000)
            .await
            .unwrap();
        assert_eq!(marker.last_read_seq, 2);
        // Stale markers leave the stored row untouched.
        let stale = service
            .set_read_marker(&project, &channel.id, &reader, 1, 2_001)
            .await
            .unwrap();
        assert_eq!(stale.last_read_seq, 2);
        assert_eq!(
            service
                .get_read_marker(&project, &channel.id, &reader)
                .await
                .unwrap()
                .unwrap()
                .last_read_seq,
            2
        );
    }

    #[test]
    fn composing_is_ephemeral_and_expires() {
        let service = CollaborationService::with_defaults(None);
        let channel = ChannelId::parse("channel-1").unwrap();
        let principal = PrincipalId::parse("principal-1").unwrap();
        let start = Instant::now();
        service
            .set_composing(&channel, &principal, "client-1", true, start, 1_000)
            .unwrap();
        assert_eq!(service.list_composing(&channel, start).len(), 1);
        // Past the TTL the lease expires through the single cleanup path.
        let later = start + Duration::from_secs(31);
        assert!(service.list_composing(&channel, later).is_empty());
        // Clearing drops everything; restart never fabricates leases.
        service
            .set_composing(&channel, &principal, "client-1", true, start, 1_000)
            .unwrap();
        service.clear_composing();
        assert!(service.list_composing(&channel, start).is_empty());
    }

    #[tokio::test(flavor = "current_thread")]
    async fn secret_bodies_persist_redacted() {
        let pool = test_pool().await;
        let service = service_with_pool(pool);
        let (project, author, _) = ids();
        let channel = service
            .ensure_default_channel(&project, &author, 1_000)
            .await
            .unwrap();
        let sent = service
            .send_message(
                &project,
                &channel.id,
                &author,
                None,
                "deploy with api_key=hunter2 now",
                None,
                None,
                &[],
                &[],
                None,
                1_001,
            )
            .await
            .unwrap()
            .message;
        assert!(sent.body.contains(REDACTED_MARKER));
        assert!(!sent.body.contains("hunter2"));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn restart_preserves_durable_order_and_markers() {
        let pool = test_pool().await;
        let project = ProjectId::parse("project-restart").unwrap();
        let author = PrincipalId::parse("principal-restart").unwrap();
        let channel_id = {
            let service = service_with_pool(pool.clone());
            let channel = service
                .ensure_default_channel(&project, &author, 1_000)
                .await
                .unwrap();
            for i in 0..3 {
                service
                    .send_message(
                        &project,
                        &channel.id,
                        &author,
                        None,
                        &format!("restart-{i}"),
                        None,
                        None,
                        &[],
                        &[],
                        None,
                        1_001 + i,
                    )
                    .await
                    .unwrap();
            }
            service
                .set_read_marker(&project, &channel.id, &author, 2, 2_000)
                .await
                .unwrap();
            channel.id.clone()
        };
        // A fresh service over the same pool observes the durable state.
        let reopened = service_with_pool(pool);
        let page = reopened
            .history(&project, &channel_id, None, Some(50))
            .await
            .unwrap();
        assert_eq!(page.messages.len(), 3);
        assert_eq!(page.messages[0].seq, 1);
        assert_eq!(page.messages[2].seq, 3);
        let marker = reopened
            .get_read_marker(&project, &channel_id, &author)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(marker.last_read_seq, 2);
        // Composing does not survive the restart.
        assert!(reopened
            .list_composing(&channel_id, Instant::now())
            .is_empty());
    }

    // ── M003: structured chat actions ────────────────────────────────

    async fn action_message(
        service: &CollaborationService,
        project: &ProjectId,
        channel: &ChannelId,
        author: &PrincipalId,
    ) -> ChatMessage {
        service
            .send_message(
                project,
                channel,
                author,
                None,
                "linked note",
                None,
                None,
                &[],
                &[],
                None,
                1_001,
            )
            .await
            .expect("send message")
            .message
    }

    #[test]
    fn action_kinds_map_to_capabilities_closed() {
        assert_eq!(
            ChatActionKind::AgentTask.required_capability(),
            "agent.delegate"
        );
        assert_eq!(
            ChatActionKind::ReviewRequest.required_capability(),
            "agent.delegate"
        );
        assert_eq!(
            ChatActionKind::JobSubmit.required_capability(),
            "job.submit"
        );
        assert_eq!(
            ChatActionKind::JobReference.required_capability(),
            "session.read"
        );
        for kind in [
            ChatActionKind::AgentTask,
            ChatActionKind::ReviewRequest,
            ChatActionKind::JobSubmit,
            ChatActionKind::JobReference,
        ] {
            assert_eq!(ChatActionKind::parse(kind.as_str()), Some(kind));
        }
        assert_eq!(ChatActionKind::parse("shell_exec"), None);
    }

    #[test]
    fn action_titles_and_prompts_reject_secrets() {
        let config = CollaborationConfig::default();
        assert!(validate_action_title(&config, Some("review the diff")).is_ok());
        assert!(validate_action_title(&config, None).unwrap().is_none());
        assert!(validate_action_title(&config, Some("  "))
            .unwrap()
            .is_none());
        assert!(validate_action_title(&config, Some("note api_key=hunter2")).is_err());
        assert!(validate_action_prompt(&config, "review this change").is_ok());
        assert!(validate_action_prompt(&config, "deploy token=abc123-secret").is_err());
        assert!(validate_action_prompt(&config, "").is_err());
        assert!(validate_action_agent(&config, "reviewer").is_ok());
        assert!(validate_action_agent(&config, "").is_err());
    }

    #[tokio::test(flavor = "current_thread")]
    async fn action_insert_finds_lists_and_rejects_mismatch() {
        let pool = test_pool().await;
        let service = service_with_pool(pool);
        let (project, author, _) = ids();
        let channel = service
            .ensure_default_channel(&project, &author, 1_000)
            .await
            .unwrap();
        let message = action_message(&service, &project, &channel.id, &author).await;

        let stored = service
            .insert_action(
                &project,
                &channel.id,
                &message.id,
                &author,
                ChatActionKind::AgentTask,
                Some("review".to_owned()),
                Some("job-1".to_owned()),
                CHAT_ACTION_STATUS_SUBMITTED,
                "action-key-1",
                2_000,
            )
            .await
            .unwrap();
        assert_eq!(stored.kind, ChatActionKind::AgentTask);
        assert_eq!(stored.status, CHAT_ACTION_STATUS_SUBMITTED);

        // Idempotency lookup converges.
        let found = service
            .find_action_by_idempotency(&project, &channel.id, "action-key-1")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(found.id, stored.id);

        // Duplicate insert with the same key conflicts (retry backstop).
        let conflict = service
            .insert_action(
                &project,
                &channel.id,
                &message.id,
                &author,
                ChatActionKind::AgentTask,
                None,
                Some("job-2".to_owned()),
                CHAT_ACTION_STATUS_SUBMITTED,
                "action-key-1",
                2_001,
            )
            .await;
        assert!(matches!(
            conflict,
            Err(CollaborationError::ActionConflict(_))
        ));

        // Cross-channel linkage is a project mismatch, never coerced.
        let other_channel = service
            .ensure_channel_by_name(&project, &author, "other", 3_000)
            .await
            .unwrap();
        let mismatch = service
            .insert_action(
                &project,
                &other_channel.id,
                &message.id,
                &author,
                ChatActionKind::JobReference,
                None,
                Some("job-1".to_owned()),
                CHAT_ACTION_STATUS_REFERENCED,
                "action-key-2",
                3_001,
            )
            .await;
        assert!(matches!(
            mismatch,
            Err(CollaborationError::ProjectMismatch(_))
        ));

        // Get and list project the stored reference without execution state.
        let fetched = service
            .get_action(&project, &channel.id, &stored.id)
            .await
            .unwrap();
        assert_eq!(fetched.job_id.as_deref(), Some("job-1"));
        let listed = service
            .list_actions(&project, &channel.id, Some(&message.id), None)
            .await
            .unwrap();
        assert_eq!(listed.len(), 1);
        let audit = audit_metadata_for_action(&stored);
        assert_eq!(
            audit.get("chat.action").map(String::as_str),
            Some(stored.id.as_str())
        );
        assert_eq!(
            audit.get("chat.action_kind").map(String::as_str),
            Some("agent_task")
        );
        // Prompts/titles never enter audit metadata.
        assert!(!audit.values().any(|v| v.contains("review")));
    }
}

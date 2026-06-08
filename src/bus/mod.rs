pub mod events;
pub mod global;

use crate::permission::PermissionChoice;
use dashmap::DashMap;
use once_cell::sync::Lazy;
use std::time::{Duration, Instant};

/// Sentinel `session_id` used by the backward-compatible
/// `PermissionRegistry::register(perm_id, tx)` and
/// `QuestionRegistry::register(question_id, tx)` calls that do not supply an
/// explicit `session_id`. New call sites should always use the
/// `*_with_session` variants with a real `session_id`.
pub const DEFAULT_SESSION_ID: &str = "default";

/// Tracks a permission request awaiting a user response. Stored in the
/// `PermissionRegistry` so that the responding call site can verify the
/// `session_id` (and optionally `turn_id`) before completing the channel.
#[derive(Debug)]
pub struct PendingPermission {
    pub session_id: String,
    pub turn_id: Option<String>,
    pub tx: tokio::sync::oneshot::Sender<PermissionChoice>,
    pub created_at: Instant,
}

/// Read-only view of a pending permission. Safe to clone, inspect, and
/// return across thread boundaries because it does not carry the oneshot
/// sender.
#[derive(Debug, Clone)]
pub struct PendingPermissionInfo {
    pub perm_id: String,
    pub session_id: String,
    pub turn_id: Option<String>,
    pub created_at: Instant,
}

/// Tracks a question awaiting a user response. Stored in the
/// `QuestionRegistry` so that the responding call site can verify the
/// `session_id` (and optionally `turn_id`) before completing the channel.
#[derive(Debug)]
pub struct PendingQuestion {
    pub session_id: String,
    pub turn_id: Option<String>,
    pub tx: tokio::sync::oneshot::Sender<String>,
    pub created_at: Instant,
}

/// Read-only view of a pending question. Safe to clone, inspect, and
/// return across thread boundaries because it does not carry the oneshot
/// sender.
#[derive(Debug, Clone)]
pub struct PendingQuestionInfo {
    pub question_id: String,
    pub session_id: String,
    pub turn_id: Option<String>,
    pub created_at: Instant,
}

static PERMISSION_REGISTRY: Lazy<PermissionRegistry> = Lazy::new(PermissionRegistry::new);

pub struct PermissionRegistry {
    senders: DashMap<String, PendingPermission>,
}

impl PermissionRegistry {
    pub fn new() -> Self {
        Self {
            senders: DashMap::new(),
        }
    }

    /// Backward-compatible registration. Assigns `session_id = "default"`
    /// and `turn_id = None`. New call sites should prefer
    /// [`Self::register_with_session`].
    pub fn register(perm_id: String, tx: tokio::sync::oneshot::Sender<PermissionChoice>) {
        Self::register_with_session(DEFAULT_SESSION_ID.to_string(), None, perm_id, tx);
    }

    /// Register a permission request with full session/turn scoping.
    ///
    /// The `session_id` and `turn_id` are stored alongside the channel so
    /// that later `respond_scoped` / `unregister_scoped` calls can verify
    /// that the responder is operating in the correct session.
    pub fn register_with_session(
        session_id: String,
        turn_id: Option<String>,
        perm_id: String,
        tx: tokio::sync::oneshot::Sender<PermissionChoice>,
    ) {
        Self::cleanup();
        let pending = PendingPermission {
            session_id,
            turn_id,
            tx,
            created_at: Instant::now(),
        };
        PERMISSION_REGISTRY.senders.insert(perm_id, pending);
    }

    /// Backward-compatible respond. Looks up by `perm_id` only, treating
    /// the registration as belonging to `session_id = "default"`. New call
    /// sites should prefer [`Self::respond_scoped`].
    pub fn respond(perm_id: String, choice: PermissionChoice) -> bool {
        Self::respond_scoped(DEFAULT_SESSION_ID, &perm_id, choice)
    }

    /// Respond to a permission, but only if it belongs to the given
    /// `session_id`. Returns `false` if `perm_id` is unknown or if the
    /// entry is owned by a different session.
    ///
    /// The check-and-remove is atomic via [`DashMap::remove_if`], so
    /// concurrent responders from another session cannot cause a
    /// double-respond.
    pub fn respond_scoped(session_id: &str, perm_id: &str, choice: PermissionChoice) -> bool {
        let removed = PERMISSION_REGISTRY
            .senders
            .remove_if(perm_id, |_key, val| val.session_id == session_id);
        if let Some((_, pending)) = removed {
            if pending.tx.send(choice).is_err() {
                tracing::warn!("failed to send permission response for {}", perm_id);
                return false;
            }
            true
        } else {
            false
        }
    }

    /// Backward-compatible unregister. Removes by `perm_id` only,
    /// regardless of session. New call sites should prefer
    /// [`Self::unregister_scoped`].
    pub fn unregister(perm_id: &str) {
        let _ = PERMISSION_REGISTRY.senders.remove(perm_id);
    }

    /// Unregister a permission, but only if it belongs to the given
    /// `session_id`. Silently no-ops if the perm is unknown or owned by a
    /// different session.
    pub fn unregister_scoped(session_id: &str, perm_id: &str) {
        let _ = PERMISSION_REGISTRY
            .senders
            .remove_if(perm_id, |_key, val| val.session_id == session_id);
    }

    /// Backward-compatible existence check. Returns `true` if `perm_id` is
    /// registered at all, in any session. New call sites should prefer
    /// [`Self::is_registered_scoped`].
    pub fn is_registered(perm_id: &str) -> bool {
        PERMISSION_REGISTRY.senders.contains_key(perm_id)
    }

    /// Returns `true` if `perm_id` is registered AND owned by the given
    /// `session_id`.
    pub fn is_registered_scoped(session_id: &str, perm_id: &str) -> bool {
        PERMISSION_REGISTRY
            .senders
            .get(perm_id)
            .map(|v| v.value().session_id == session_id)
            .unwrap_or(false)
    }

    pub fn pending_permission_ids() -> Vec<String> {
        Self::cleanup();
        PERMISSION_REGISTRY
            .senders
            .iter()
            .map(|entry| entry.key().clone())
            .collect()
    }

    /// Return all pending permissions for the given `session_id`, with
    /// full metadata (perm_id, session_id, turn_id, created_at).
    pub fn get_pending_for_session(session_id: &str) -> Vec<PendingPermissionInfo> {
        Self::cleanup();
        PERMISSION_REGISTRY
            .senders
            .iter()
            .filter(|e| e.value().session_id == session_id)
            .map(|e| PendingPermissionInfo {
                perm_id: e.key().clone(),
                session_id: e.value().session_id.clone(),
                turn_id: e.value().turn_id.clone(),
                created_at: e.value().created_at,
            })
            .collect()
    }

    fn cleanup() {
        let ttl = Duration::from_secs(310);
        PERMISSION_REGISTRY
            .senders
            .retain(|_, pending| pending.created_at.elapsed() < ttl);
    }

    pub fn cleanup_now() {
        Self::cleanup();
    }
}

impl Default for PermissionRegistry {
    fn default() -> Self {
        Self::new()
    }
}

static QUESTION_REGISTRY: Lazy<QuestionRegistry> = Lazy::new(QuestionRegistry::new);

pub struct QuestionRegistry {
    senders: DashMap<String, PendingQuestion>,
}

impl QuestionRegistry {
    pub fn new() -> Self {
        Self {
            senders: DashMap::new(),
        }
    }

    /// Backward-compatible registration. Assigns `session_id = "default"`
    /// and `turn_id = None`. New call sites should prefer
    /// [`Self::register_with_session`].
    pub fn register(question_id: String, tx: tokio::sync::oneshot::Sender<String>) {
        Self::register_with_session(DEFAULT_SESSION_ID.to_string(), None, question_id, tx);
    }

    /// Register a question with full session/turn scoping.
    ///
    /// The `session_id` and `turn_id` are stored alongside the channel so
    /// that later `answer_question_scoped` / `unregister_scoped` calls can
    /// verify that the responder is operating in the correct session.
    pub fn register_with_session(
        session_id: String,
        turn_id: Option<String>,
        question_id: String,
        tx: tokio::sync::oneshot::Sender<String>,
    ) {
        Self::cleanup();
        let pending = PendingQuestion {
            session_id,
            turn_id,
            tx,
            created_at: Instant::now(),
        };
        QUESTION_REGISTRY.senders.insert(question_id, pending);
    }

    /// Backward-compatible answer. Looks up by `question_id` only,
    /// treating the registration as belonging to `session_id = "default"`.
    /// New call sites should prefer [`Self::answer_question_scoped`].
    pub fn answer_question(question_id: String, answers: String) -> bool {
        Self::answer_question_scoped(DEFAULT_SESSION_ID, &question_id, answers)
    }

    /// Answer a question, but only if it belongs to the given
    /// `session_id`. Returns `false` if `question_id` is unknown or owned
    /// by a different session.
    ///
    /// The check-and-remove is atomic via [`DashMap::remove_if`].
    pub fn answer_question_scoped(session_id: &str, question_id: &str, answers: String) -> bool {
        let removed = QUESTION_REGISTRY
            .senders
            .remove_if(question_id, |_key, val| val.session_id == session_id);
        if let Some((_, pending)) = removed {
            if pending.tx.send(answers).is_err() {
                tracing::warn!("failed to send question answer for {}", question_id);
                return false;
            }
            true
        } else {
            false
        }
    }

    /// Backward-compatible unregister. Removes by `question_id` only,
    /// regardless of session. New call sites should prefer
    /// [`Self::unregister_scoped`].
    pub fn unregister(question_id: &str) {
        let _ = QUESTION_REGISTRY.senders.remove(question_id);
    }

    /// Unregister a question, but only if it belongs to the given
    /// `session_id`. Silently no-ops if the question is unknown or owned
    /// by a different session.
    pub fn unregister_scoped(session_id: &str, question_id: &str) {
        let _ = QUESTION_REGISTRY
            .senders
            .remove_if(question_id, |_key, val| val.session_id == session_id);
    }

    /// Backward-compatible existence check. Returns `true` if
    /// `question_id` is registered at all, in any session. New call sites
    /// should prefer [`Self::is_registered_scoped`].
    pub fn is_registered(question_id: &str) -> bool {
        QUESTION_REGISTRY.senders.contains_key(question_id)
    }

    /// Returns `true` if `question_id` is registered AND owned by the
    /// given `session_id`.
    pub fn is_registered_scoped(session_id: &str, question_id: &str) -> bool {
        QUESTION_REGISTRY
            .senders
            .get(question_id)
            .map(|v| v.value().session_id == session_id)
            .unwrap_or(false)
    }

    pub fn pending_question_ids() -> Vec<String> {
        Self::cleanup();
        QUESTION_REGISTRY
            .senders
            .iter()
            .map(|entry| entry.key().clone())
            .collect()
    }

    /// Return all pending questions for the given `session_id`, with full
    /// metadata (question_id, session_id, turn_id, created_at).
    pub fn get_pending_for_session(session_id: &str) -> Vec<PendingQuestionInfo> {
        Self::cleanup();
        QUESTION_REGISTRY
            .senders
            .iter()
            .filter(|e| e.value().session_id == session_id)
            .map(|e| PendingQuestionInfo {
                question_id: e.key().clone(),
                session_id: e.value().session_id.clone(),
                turn_id: e.value().turn_id.clone(),
                created_at: e.value().created_at,
            })
            .collect()
    }

    fn cleanup() {
        let ttl = Duration::from_secs(310);
        QUESTION_REGISTRY
            .senders
            .retain(|_, pending| pending.created_at.elapsed() < ttl);
    }

    pub fn cleanup_now() {
        Self::cleanup();
    }
}

impl Default for QuestionRegistry {
    fn default() -> Self {
        Self::new()
    }
}

pub mod events;
pub mod global;

use dashmap::DashMap;
use once_cell::sync::Lazy;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

/// User-facing permission decision, owned by the bus layer so that the
/// low-level bus module does not depend on the `permission` domain module.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PermissionDecision {
    AllowOnce,
    AlwaysAllow,
    DenyOnce,
    AlwaysDeny,
}

impl PermissionDecision {
    pub fn allowed(&self) -> bool {
        matches!(
            self,
            PermissionDecision::AllowOnce | PermissionDecision::AlwaysAllow
        )
    }

    pub fn persist(&self) -> bool {
        matches!(
            self,
            PermissionDecision::AlwaysAllow | PermissionDecision::AlwaysDeny
        )
    }
}

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
    pub tx: tokio::sync::oneshot::Sender<PermissionDecision>,
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
    last_cleanup_ms: AtomicU64,
    /// Upper bound on how long an unanswered request may sit in the map.
    ///
    /// `None` (the default) means entries never expire on age alone. The
    /// waiter unregisters on every exit path, so age-based eviction is only a
    /// leak guard for a waiter that was dropped mid-await (a cancelled turn).
    /// A fixed 310s TTL was previously the *only* expiry, which silently
    /// denied any approval the user simply had not answered yet — including
    /// with the router configured to wait indefinitely.
    ///
    /// When the caller sets a real approval timeout it must also raise this
    /// above that timeout, so the router's own timeout wins and unregisters
    /// cleanly rather than the map dropping the sender out from under it.
    pending_ttl: AtomicU64,
}

/// Sentinel for [`PermissionRegistry::pending_ttl`]: no age-based expiry.
const PENDING_TTL_UNBOUNDED: u64 = u64::MAX;

impl PermissionRegistry {
    pub fn new() -> Self {
        Self {
            senders: DashMap::new(),
            last_cleanup_ms: AtomicU64::new(0),
            pending_ttl: AtomicU64::new(PENDING_TTL_UNBOUNDED),
        }
    }

    /// Bound how long an unanswered request survives in the registry.
    ///
    /// `None` disables age-based eviction entirely. Production callers pass
    /// `Some`: either the configured approval timeout plus a grace margin (so
    /// the router's timeout fires first and the waiter unregisters itself), or
    /// a long orphan guard when no timeout is configured. Evicting an entry
    /// drops its sender, so a waiter that can never be woken resolves as a
    /// closed-channel deny instead of parking forever.
    pub fn set_pending_ttl(ttl: Option<std::time::Duration>) {
        let encoded = match ttl {
            Some(d) => d.as_millis().min(u128::from(u64::MAX - 1)) as u64,
            None => PENDING_TTL_UNBOUNDED,
        };
        PERMISSION_REGISTRY
            .pending_ttl
            .store(encoded, Ordering::Release);
    }

    /// The currently configured age bound, or `None` when unbounded.
    pub fn pending_ttl() -> Option<std::time::Duration> {
        let encoded = PERMISSION_REGISTRY.pending_ttl.load(Ordering::Acquire);
        if encoded == PENDING_TTL_UNBOUNDED {
            None
        } else {
            Some(std::time::Duration::from_millis(encoded))
        }
    }

    /// Backward-compatible registration. Assigns `session_id = "default"`
    /// and `turn_id = None`. New call sites should prefer
    /// [`Self::register_with_session`].
    pub fn register(perm_id: String, tx: tokio::sync::oneshot::Sender<PermissionDecision>) {
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
        tx: tokio::sync::oneshot::Sender<PermissionDecision>,
    ) {
        Self::cleanup_if_due();
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
    pub fn respond(perm_id: String, choice: PermissionDecision) -> bool {
        Self::respond_scoped(DEFAULT_SESSION_ID, &perm_id, choice)
    }

    /// Respond to a permission, but only if it belongs to the given
    /// `session_id`. Returns `false` if `perm_id` is unknown or if the
    /// entry is owned by a different session.
    ///
    /// The check-and-remove is atomic via [`DashMap::remove_if`], so
    /// concurrent responders from another session cannot cause a
    /// double-respond.
    pub fn respond_scoped(session_id: &str, perm_id: &str, choice: PermissionDecision) -> bool {
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

    /// Backward-compatible unregister. Removes only entries owned by
    /// `session_id = "default"`, mirroring [`Self::respond`]. New call
    /// sites should prefer [`Self::unregister_scoped`].
    pub fn unregister(perm_id: &str) {
        Self::unregister_scoped(DEFAULT_SESSION_ID, perm_id);
    }

    /// Unregister a permission, but only if it belongs to the given
    /// `session_id`. Silently no-ops if the perm is unknown or owned by a
    /// different session.
    pub fn unregister_scoped(session_id: &str, perm_id: &str) {
        let _ = PERMISSION_REGISTRY
            .senders
            .remove_if(perm_id, |_key, val| val.session_id == session_id);
    }

    /// Drop every pending permission sender owned by a deleted session.
    /// Dropping the senders wakes their waiters with a closed-channel deny.
    pub fn cancel_session(session_id: &str) -> usize {
        let ids = PERMISSION_REGISTRY
            .senders
            .iter()
            .filter(|entry| entry.value().session_id == session_id)
            .map(|entry| entry.key().clone())
            .collect::<Vec<_>>();
        ids.into_iter()
            .filter(|perm_id| {
                PERMISSION_REGISTRY
                    .senders
                    .remove_if(perm_id, |_key, val| val.session_id == session_id)
                    .is_some()
            })
            .count()
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
        Self::cleanup_if_due();
        PERMISSION_REGISTRY
            .senders
            .iter()
            .map(|entry| entry.key().clone())
            .collect()
    }

    /// Return all pending permissions for the given `session_id`, with
    /// full metadata (perm_id, session_id, turn_id, created_at).
    pub fn get_pending_for_session(session_id: &str) -> Vec<PendingPermissionInfo> {
        Self::cleanup_if_due();
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

    /// Resolve the owning `(session_id, turn_id)` of a pending permission
    /// addressed by its **simple** (unprefixed) id.
    ///
    /// The daemon's `PermissionRespond` handler accepts both the scoped
    /// `perm:<session>:<turn>:<simple>` protocol id and the bare simple id
    /// that the local TUI receives verbatim from
    /// `AppEvent::PermissionPending`. The bare form carries no session, so
    /// ownership has to be recovered from the registry itself.
    ///
    /// Fails closed (`None`) when the id is unknown **or** when more than one
    /// session currently owns the same simple id — an ambiguous answer must
    /// never be routed to an arbitrary waiter.
    pub fn resolve_pending_owner(perm_id: &str) -> Option<(String, Option<String>)> {
        Self::cleanup_if_due();
        let mut matches = PERMISSION_REGISTRY
            .senders
            .iter()
            .filter(|e| e.key().as_str() == perm_id)
            .map(|e| (e.value().session_id.clone(), e.value().turn_id.clone()));
        let first = matches.next()?;
        // A second owner with the same simple id makes the answer ambiguous.
        matches.next().is_none().then_some(first)
    }

    fn cleanup_if_due() {
        let Some(ttl) = PermissionRegistry::pending_ttl() else {
            return;
        };
        let now = monotonic_millis();
        let last = PERMISSION_REGISTRY.last_cleanup_ms.load(Ordering::Relaxed);
        if now.saturating_sub(last) < CLEANUP_THROTTLE_MS
            || PERMISSION_REGISTRY
                .last_cleanup_ms
                .compare_exchange(last, now, Ordering::AcqRel, Ordering::Relaxed)
                .is_err()
        {
            return;
        }
        PERMISSION_REGISTRY
            .senders
            .retain(|_, pending| pending.created_at.elapsed() < ttl);
    }

    /// Sweep expired entries immediately, ignoring the 30s throttle.
    ///
    /// A no-op while the TTL is unbounded.
    pub fn cleanup_now() {
        let Some(ttl) = PermissionRegistry::pending_ttl() else {
            return;
        };
        PERMISSION_REGISTRY
            .last_cleanup_ms
            .store(monotonic_millis(), Ordering::Release);
        PERMISSION_REGISTRY
            .senders
            .retain(|_, pending| pending.created_at.elapsed() < ttl);
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
    last_cleanup_ms: AtomicU64,
}

impl QuestionRegistry {
    pub fn new() -> Self {
        Self {
            senders: DashMap::new(),
            last_cleanup_ms: AtomicU64::new(0),
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
        Self::cleanup_if_due();
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

    /// Backward-compatible unregister. Removes only entries owned by
    /// `session_id = "default"`, mirroring [`Self::answer_question`]. New
    /// call sites should prefer [`Self::unregister_scoped`].
    pub fn unregister(question_id: &str) {
        Self::unregister_scoped(DEFAULT_SESSION_ID, question_id);
    }

    /// Unregister a question, but only if it belongs to the given
    /// `session_id`. Silently no-ops if the question is unknown or owned
    /// by a different session.
    pub fn unregister_scoped(session_id: &str, question_id: &str) {
        let _ = QUESTION_REGISTRY
            .senders
            .remove_if(question_id, |_key, val| val.session_id == session_id);
    }

    /// Drop every pending question sender owned by a deleted session.
    /// Dropping the senders wakes their waiters with a closed-channel error.
    pub fn cancel_session(session_id: &str) -> usize {
        let ids = QUESTION_REGISTRY
            .senders
            .iter()
            .filter(|entry| entry.value().session_id == session_id)
            .map(|entry| entry.key().clone())
            .collect::<Vec<_>>();
        ids.into_iter()
            .filter(|question_id| {
                QUESTION_REGISTRY
                    .senders
                    .remove_if(question_id, |_key, val| val.session_id == session_id)
                    .is_some()
            })
            .count()
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
        Self::cleanup_if_due();
        QUESTION_REGISTRY
            .senders
            .iter()
            .map(|entry| entry.key().clone())
            .collect()
    }

    /// Resolve the owning `(session_id, turn_id)` of a pending question
    /// addressed by its **simple** (unprefixed) id.
    ///
    /// Mirrors [`PermissionRegistry::resolve_pending_owner`]: the bare id
    /// carries no session, so ownership is recovered from the registry, and
    /// an ambiguous id (more than one owner) fails closed.
    pub fn resolve_pending_owner(question_id: &str) -> Option<(String, Option<String>)> {
        Self::cleanup_if_due();
        let mut matches = QUESTION_REGISTRY
            .senders
            .iter()
            .filter(|e| e.key().as_str() == question_id)
            .map(|e| (e.value().session_id.clone(), e.value().turn_id.clone()));
        let first = matches.next()?;
        matches.next().is_none().then_some(first)
    }

    /// Return all pending questions for the given `session_id`, with full
    /// metadata (question_id, session_id, turn_id, created_at).
    pub fn get_pending_for_session(session_id: &str) -> Vec<PendingQuestionInfo> {
        Self::cleanup_if_due();
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

    fn cleanup_if_due() {
        let ttl = Duration::from_secs(310);
        let now = monotonic_millis();
        let last = QUESTION_REGISTRY.last_cleanup_ms.load(Ordering::Relaxed);
        if now.saturating_sub(last) < CLEANUP_THROTTLE_MS
            || QUESTION_REGISTRY
                .last_cleanup_ms
                .compare_exchange(last, now, Ordering::AcqRel, Ordering::Relaxed)
                .is_err()
        {
            return;
        }
        QUESTION_REGISTRY
            .senders
            .retain(|_, pending| pending.created_at.elapsed() < ttl);
    }

    pub fn cleanup_now() {
        QUESTION_REGISTRY
            .last_cleanup_ms
            .store(monotonic_millis(), Ordering::Release);
        QUESTION_REGISTRY
            .senders
            .retain(|_, pending| pending.created_at.elapsed() < Duration::from_secs(310));
    }
}

/// Cleanup throttle measured in monotonic milliseconds since process
/// start. Wall-clock time is NTP-jump sensitive and must not gate the
/// TTL sweep (which itself uses `Instant`).
const CLEANUP_THROTTLE_MS: u64 = 30_000;

fn monotonic_millis() -> u64 {
    static START: Lazy<Instant> = Lazy::new(Instant::now);
    START.elapsed().as_millis() as u64
}

impl Default for QuestionRegistry {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The permission registry is a process global, so this restores whatever
    /// it found. Runs in one process alongside other bus tests under
    /// `cargo test --lib`.
    struct TtlGuard(Option<Duration>);

    impl Drop for TtlGuard {
        fn drop(&mut self) {
            PermissionRegistry::set_pending_ttl(self.0);
        }
    }

    #[test]
    fn pending_ttl_is_unbounded_by_default() {
        let _guard = TtlGuard(PermissionRegistry::pending_ttl());
        PermissionRegistry::set_pending_ttl(None);

        // A pending approval must survive indefinitely on age alone. The old
        // fixed 310s sweep evicted live requests, so a user who simply had
        // not answered yet saw the tool denied.
        assert_eq!(PermissionRegistry::pending_ttl(), None);

        let (tx, mut rx) = tokio::sync::oneshot::channel();
        PermissionRegistry::register_with_session("ttl-s".into(), None, "ttl-p".into(), tx);
        PermissionRegistry::cleanup_now();
        assert!(
            PermissionRegistry::respond_scoped("ttl-s", "ttl-p", PermissionDecision::AllowOnce),
            "unbounded registry must not evict a pending request"
        );
        assert_eq!(rx.try_recv().ok(), Some(PermissionDecision::AllowOnce));
    }

    #[test]
    fn pending_ttl_round_trips_a_configured_bound() {
        let _guard = TtlGuard(PermissionRegistry::pending_ttl());

        PermissionRegistry::set_pending_ttl(Some(Duration::from_secs(960)));
        assert_eq!(
            PermissionRegistry::pending_ttl(),
            Some(Duration::from_secs(960))
        );

        PermissionRegistry::set_pending_ttl(None);
        assert_eq!(PermissionRegistry::pending_ttl(), None);
    }

    #[test]
    fn cancel_session_closes_only_its_pending_permissions() {
        let session_id = format!("delete-perm-{}", uuid::Uuid::new_v4());
        let other_session = format!("keep-perm-{}", uuid::Uuid::new_v4());
        let (tx, mut rx) = tokio::sync::oneshot::channel();
        let (other_tx, mut other_rx) = tokio::sync::oneshot::channel();
        PermissionRegistry::register_with_session(
            session_id.clone(),
            None,
            format!("{}-id", session_id),
            tx,
        );
        PermissionRegistry::register_with_session(
            other_session.clone(),
            None,
            format!("{}-id", other_session),
            other_tx,
        );

        assert_eq!(PermissionRegistry::cancel_session(&session_id), 1);
        assert_eq!(
            rx.try_recv(),
            Err(tokio::sync::oneshot::error::TryRecvError::Closed)
        );
        assert!(PermissionRegistry::is_registered_scoped(
            &other_session,
            &format!("{}-id", other_session)
        ));
        PermissionRegistry::cancel_session(&other_session);
        assert_eq!(
            other_rx.try_recv(),
            Err(tokio::sync::oneshot::error::TryRecvError::Closed)
        );
    }

    #[test]
    fn cancel_session_closes_only_its_pending_questions() {
        let session_id = format!("delete-question-{}", uuid::Uuid::new_v4());
        let other_session = format!("keep-question-{}", uuid::Uuid::new_v4());
        let (tx, mut rx) = tokio::sync::oneshot::channel();
        let (other_tx, mut other_rx) = tokio::sync::oneshot::channel();
        QuestionRegistry::register_with_session(
            session_id.clone(),
            None,
            format!("{}-id", session_id),
            tx,
        );
        QuestionRegistry::register_with_session(
            other_session.clone(),
            None,
            format!("{}-id", other_session),
            other_tx,
        );

        assert_eq!(QuestionRegistry::cancel_session(&session_id), 1);
        assert_eq!(
            rx.try_recv(),
            Err(tokio::sync::oneshot::error::TryRecvError::Closed)
        );
        assert!(QuestionRegistry::is_registered_scoped(
            &other_session,
            &format!("{}-id", other_session)
        ));
        QuestionRegistry::cancel_session(&other_session);
        assert_eq!(
            other_rx.try_recv(),
            Err(tokio::sync::oneshot::error::TryRecvError::Closed)
        );
    }
}

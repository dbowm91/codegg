//! Explicit ownership for desktop project-event forwarders.
//!
//! One active subscription exists per connection generation. Every forwarder
//! task has a stored owner (`JoinHandle`) that is cancelled and joined on
//! replace, unsubscribe, disconnect, or reconnect. Terminal cleanup clears
//! the stored owner only when the terminating subscription id still matches,
//! so a stale task can never clear a newer subscription.

use std::sync::atomic::{AtomicU64, Ordering};
use tokio::sync::Mutex;
use tokio::task::JoinHandle;

/// Outcome of an identity-scoped unsubscribe request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnsubscribeOutcome {
    /// The matching active subscription was cancelled and joined.
    Removed,
    /// The id matches the most recently terminated subscription; no-op success.
    AlreadyEnded,
    /// The id matches neither the active nor the last terminated subscription.
    Stale,
}

struct OwnedSubscription {
    subscription_id: String,
    connection_generation: u64,
    task: JoinHandle<()>,
}

struct RegistryInner {
    active: Option<OwnedSubscription>,
    last_terminated_id: Option<String>,
}

/// Testable owner for desktop project-event forwarder tasks.
///
/// The registry never holds its mutex guard across a task join: every method
/// takes the previous owner out under the lock, drops the guard, then aborts
/// and awaits the task. Forwarder exit paths use [`Self::clear_if_matching`],
/// which is synchronous and compare-by-id.
pub struct SubscriptionRegistry {
    next_id: AtomicU64,
    inner: Mutex<RegistryInner>,
}

impl Default for SubscriptionRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl SubscriptionRegistry {
    pub fn new() -> Self {
        Self {
            next_id: AtomicU64::new(1),
            inner: Mutex::new(RegistryInner {
                active: None,
                last_terminated_id: None,
            }),
        }
    }

    /// Number of currently owned forwarder tasks (0 or 1).
    #[allow(dead_code)]
    pub async fn active_count(&self) -> usize {
        usize::from(self.inner.lock().await.active.is_some())
    }

    /// Snapshot of the current owner, if any.
    pub async fn active_snapshot(&self) -> Option<(String, u64)> {
        let guard = self.inner.lock().await;
        guard
            .active
            .as_ref()
            .map(|owned| (owned.subscription_id.clone(), owned.connection_generation))
    }

    /// Allocate a process-unique subscription identity without installing it.
    ///
    /// Callers allocate first, spawn the forwarder bound to that id, then call
    /// [`Self::install_with_id`] so a task that exits before installation
    /// cannot be mistaken for the new owner.
    pub fn allocate_subscription_id(&self) -> String {
        let seq = self.next_id.fetch_add(1, Ordering::AcqRel);
        format!("desktop-sub-{seq}")
    }

    /// Atomically replace the active subscription with a pre-allocated id.
    ///
    /// The previous owner, if any, is cancelled and joined after the swap so
    /// at most one owner is ever stored. Returns the installed id.
    pub async fn install_with_id(
        &self,
        subscription_id: String,
        connection_generation: u64,
        task: JoinHandle<()>,
    ) -> String {
        let previous = {
            let mut guard = self.inner.lock().await;
            let previous = guard.active.take();
            guard.active = Some(OwnedSubscription {
                subscription_id: subscription_id.clone(),
                connection_generation,
                task,
            });
            previous
        };
        if let Some(previous) = previous {
            // Replacement supersedes the old subscription; report a later
            // unsubscribe for the superseded id as stale (still success,
            // never touching the new owner). Explicit unsubscribe/shutdown
            // paths record last_terminated_id for idempotent repeats.
            cancel_and_join(previous.task).await;
        }
        subscription_id
    }

    /// Convenience wrapper that allocates an id and installs the task.
    #[allow(dead_code)]
    pub async fn install(&self, connection_generation: u64, task: JoinHandle<()>) -> String {
        let id = self.allocate_subscription_id();
        self.install_with_id(id, connection_generation, task).await
    }

    /// Cancel and join only the subscription matching `subscription_id`.
    ///
    /// A stale id never affects the current owner. Repeating an unsubscribe
    /// for the most recently terminated id succeeds without work.
    pub async fn unsubscribe(&self, subscription_id: &str) -> UnsubscribeOutcome {
        let owned = {
            let mut guard = self.inner.lock().await;
            match guard.active.as_ref() {
                Some(active) if active.subscription_id == subscription_id => guard.active.take(),
                _ => {
                    if guard.last_terminated_id.as_deref() == Some(subscription_id) {
                        return UnsubscribeOutcome::AlreadyEnded;
                    }
                    return UnsubscribeOutcome::Stale;
                }
            }
        };
        if let Some(owned) = owned {
            let id = owned.subscription_id.clone();
            cancel_and_join(owned.task).await;
            let mut guard = self.inner.lock().await;
            guard.last_terminated_id = Some(id);
            UnsubscribeOutcome::Removed
        } else {
            UnsubscribeOutcome::Stale
        }
    }

    /// Clear the stored owner only if it still matches `subscription_id`.
    ///
    /// Used by terminating forwarder tasks (channel send failure, event stream
    /// close, generation mismatch). Never awaits and never touches a newer
    /// owner. Returns true when an owner was cleared.
    pub async fn clear_if_matching(&self, subscription_id: &str) -> bool {
        let mut guard = self.inner.lock().await;
        match guard.active.as_ref() {
            Some(active) if active.subscription_id == subscription_id => {
                let owned = guard.active.take();
                if let Some(owned) = owned {
                    guard.last_terminated_id = Some(owned.subscription_id);
                }
                true
            }
            _ => false,
        }
    }

    /// Take the active owner for disconnect/reconnect teardown.
    ///
    /// The caller must abort and join the returned task without holding the
    /// registry lock; prefer [`Self::shutdown`] which does both.
    pub async fn shutdown(&self) {
        let owned = {
            let mut guard = self.inner.lock().await;
            guard.active.take()
        };
        if let Some(owned) = owned {
            let id = owned.subscription_id.clone();
            cancel_and_join(owned.task).await;
            let mut guard = self.inner.lock().await;
            guard.last_terminated_id = Some(id);
        }
    }
}

/// Abort a forwarder task and await its termination.
///
/// Abort is the cancellation signal; awaiting the handle provides deterministic
/// terminal observation and releases the task's `LocalSocketClient` clone.
async fn cancel_and_join(task: JoinHandle<()>) {
    task.abort();
    let _ = task.await;
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    };
    use std::time::Duration;

    struct LiveGuard {
        live: Arc<AtomicUsize>,
    }

    impl Drop for LiveGuard {
        fn drop(&mut self) {
            self.live.fetch_sub(1, Ordering::AcqRel);
        }
    }

    fn spawn_tracked(live: Arc<AtomicUsize>) -> JoinHandle<()> {
        live.fetch_add(1, Ordering::AcqRel);
        let guard = LiveGuard {
            live: Arc::clone(&live),
        };
        tokio::spawn(async move {
            // Hold the guard for the task lifetime; abort drops it promptly.
            let _guard = guard;
            std::future::pending::<()>().await;
        })
    }

    fn spawn_immediate() -> JoinHandle<()> {
        tokio::spawn(async {})
    }

    async fn settle(live: &Arc<AtomicUsize>) {
        for _ in 0..100 {
            if live.load(Ordering::Acquire) == 0 {
                return;
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    }

    #[tokio::test]
    async fn subscribe_then_unsubscribe_releases_owner() {
        let registry = SubscriptionRegistry::new();
        let live = Arc::new(AtomicUsize::new(0));
        let id = registry.install(1, spawn_tracked(Arc::clone(&live))).await;
        assert_eq!(registry.active_count().await, 1);
        assert_eq!(registry.active_snapshot().await, Some((id.clone(), 1)));
        assert_eq!(registry.unsubscribe(&id).await, UnsubscribeOutcome::Removed);
        assert_eq!(registry.active_count().await, 0);
        settle(&live).await;
        assert_eq!(live.load(Ordering::Acquire), 0);
        // Idempotent repeat for the same id.
        assert_eq!(
            registry.unsubscribe(&id).await,
            UnsubscribeOutcome::AlreadyEnded
        );
    }

    #[tokio::test]
    async fn subscribe_twice_cannot_accumulate_two_owners() {
        let registry = SubscriptionRegistry::new();
        let live = Arc::new(AtomicUsize::new(0));
        let first = registry.install(1, spawn_tracked(Arc::clone(&live))).await;
        let second = registry.install(1, spawn_tracked(Arc::clone(&live))).await;
        assert_ne!(first, second);
        assert_eq!(registry.active_count().await, 1);
        assert_eq!(registry.active_snapshot().await, Some((second.clone(), 1)));
        settle(&live).await;
        // Only the second task remains alive; the replaced task was joined.
        assert_eq!(live.load(Ordering::Acquire), 1);
        assert_eq!(
            registry.unsubscribe(&second).await,
            UnsubscribeOutcome::Removed
        );
        settle(&live).await;
        assert_eq!(live.load(Ordering::Acquire), 0);
    }

    #[tokio::test]
    async fn stale_unsubscribe_leaves_newer_subscription_alive() {
        let registry = SubscriptionRegistry::new();
        let live = Arc::new(AtomicUsize::new(0));
        let first = registry.install(1, spawn_tracked(Arc::clone(&live))).await;
        let second = registry.install(2, spawn_tracked(Arc::clone(&live))).await;
        assert_eq!(
            registry.unsubscribe(&first).await,
            UnsubscribeOutcome::Stale
        );
        assert_eq!(registry.active_snapshot().await, Some((second.clone(), 2)));
        assert_eq!(registry.active_count().await, 1);
        assert_eq!(
            registry.unsubscribe(&second).await,
            UnsubscribeOutcome::Removed
        );
        settle(&live).await;
        assert_eq!(live.load(Ordering::Acquire), 0);
    }

    #[tokio::test]
    async fn old_terminal_cleanup_cannot_clear_newer_owner() {
        let registry = SubscriptionRegistry::new();
        // Terminal cleanup is invoked by an exiting forwarder just before it
        // returns, so model it with already-completed tasks: dropping the
        // stored handle releases ownership without needing a join.
        let first = registry.install(1, spawn_immediate()).await;
        // Let the first task finish so replacement join is deterministic.
        tokio::time::sleep(Duration::from_millis(10)).await;
        let second = registry.install(2, spawn_immediate()).await;
        tokio::time::sleep(Duration::from_millis(10)).await;
        // Stale task exit must not clear the replacement.
        assert!(!registry.clear_if_matching(&first).await);
        assert_eq!(registry.active_snapshot().await, Some((second.clone(), 2)));
        // Matching terminal cleanup clears exactly once.
        assert!(registry.clear_if_matching(&second).await);
        assert_eq!(registry.active_count().await, 0);
        assert!(!registry.clear_if_matching(&second).await);
        // Shutdown is a no-op with no owner.
        registry.shutdown().await;
        // Re-install then shut down exercises the join path; repeat
        // unsubscribe stays idempotent.
        let live = Arc::new(AtomicUsize::new(0));
        let third = registry.install(3, spawn_tracked(Arc::clone(&live))).await;
        registry.shutdown().await;
        assert_eq!(registry.active_count().await, 0);
        assert_eq!(
            registry.unsubscribe(&third).await,
            UnsubscribeOutcome::AlreadyEnded
        );
        settle(&live).await;
        assert_eq!(live.load(Ordering::Acquire), 0);
    }

    #[tokio::test]
    async fn disconnect_tears_down_subscription() {
        let registry = SubscriptionRegistry::new();
        let live = Arc::new(AtomicUsize::new(0));
        let id = registry.install(7, spawn_tracked(Arc::clone(&live))).await;
        registry.shutdown().await;
        assert_eq!(registry.active_count().await, 0);
        assert_eq!(
            registry.unsubscribe(&id).await,
            UnsubscribeOutcome::AlreadyEnded
        );
        settle(&live).await;
        assert_eq!(live.load(Ordering::Acquire), 0);
    }

    #[tokio::test]
    async fn channel_failure_clears_matching_owner() {
        let registry = SubscriptionRegistry::new();
        let id = registry.install(4, spawn_immediate()).await;
        tokio::time::sleep(Duration::from_millis(10)).await;
        assert!(registry.clear_if_matching(&id).await);
        assert_eq!(registry.active_count().await, 0);
        assert_eq!(
            registry.unsubscribe(&id).await,
            UnsubscribeOutcome::AlreadyEnded
        );
    }

    #[tokio::test]
    async fn repeated_lifecycle_cycles_return_to_baseline() {
        let registry = SubscriptionRegistry::new();
        let live = Arc::new(AtomicUsize::new(0));
        for cycle in 0..50u64 {
            let generation = (cycle % 5) + 1;
            let id = registry
                .install(generation, spawn_tracked(Arc::clone(&live)))
                .await;
            assert_eq!(registry.active_count().await, 1);
            if cycle % 2 == 0 {
                assert_eq!(registry.unsubscribe(&id).await, UnsubscribeOutcome::Removed);
            } else {
                registry.shutdown().await;
            }
            assert_eq!(registry.active_count().await, 0);
        }
        settle(&live).await;
        assert_eq!(live.load(Ordering::Acquire), 0);
        assert_eq!(registry.active_count().await, 0);
    }

    #[tokio::test]
    async fn failed_reconnect_semantics_preserve_current_owner() {
        // Models lib.rs reconnect: a failed attempt must not touch the registry.
        let registry = SubscriptionRegistry::new();
        let live = Arc::new(AtomicUsize::new(0));
        let id = registry.install(9, spawn_tracked(Arc::clone(&live))).await;
        // No registry call on failure: owner is untouched.
        assert_eq!(registry.active_snapshot().await, Some((id.clone(), 9)));
        assert_eq!(registry.unsubscribe(&id).await, UnsubscribeOutcome::Removed);
        settle(&live).await;
        assert_eq!(live.load(Ordering::Acquire), 0);
    }
}

use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use std::sync::Arc;
use tokio::sync::{Mutex, RwLock};

use crate::bus::events::AppEvent;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginEventSubscription {
    pub plugin_id: String,
    pub event_patterns: Vec<String>,
    pub priority: i32,
}

pub struct PluginEventBus {
    subscriptions: Arc<RwLock<Vec<PluginEventSubscription>>>,
    event_log: Arc<RwLock<VecDeque<AppEvent>>>,
    publish_lock: Arc<Mutex<()>>,
    max_log_size: usize,
}

impl PluginEventBus {
    pub fn new(max_log_size: usize) -> Self {
        Self {
            subscriptions: Arc::new(RwLock::new(Vec::new())),
            event_log: Arc::new(RwLock::new(VecDeque::new())),
            publish_lock: Arc::new(Mutex::new(())),
            max_log_size,
        }
    }

    pub async fn subscribe(&self, subscription: PluginEventSubscription) {
        self.subscriptions.write().await.push(subscription);
    }

    pub async fn unsubscribe(&self, plugin_id: &str) {
        self.subscriptions
            .write()
            .await
            .retain(|s| s.plugin_id != plugin_id);
    }

    pub async fn publish(&self, event: AppEvent) {
        let _publish_guard = self.publish_lock.lock().await;
        {
            let mut log = self.event_log.write().await;
            log.push_back(event.clone());
            if log.len() > self.max_log_size {
                log.pop_front();
            }
        }

        let event_type = event.event_type();
        let subscribers = self.subscriptions.read().await;
        for sub in subscribers.iter() {
            if sub.event_patterns.is_empty()
                || sub
                    .event_patterns
                    .iter()
                    .any(|p| event_type.contains(p.as_str()))
            {
                tracing::debug!(
                    plugin = sub.plugin_id,
                    event = event_type,
                    "plugin event matched subscription"
                );
            }
        }
    }

    pub async fn subscriptions(&self) -> Vec<PluginEventSubscription> {
        self.subscriptions.read().await.clone()
    }
}

impl Default for PluginEventBus {
    fn default() -> Self {
        Self::new(1000)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test(flavor = "current_thread")]
    async fn event_log_is_bounded_and_retains_newest_events() {
        let bus = PluginEventBus::new(2);
        bus.publish(AppEvent::Error {
            message: "first".into(),
        })
        .await;
        bus.publish(AppEvent::Error {
            message: "second".into(),
        })
        .await;
        bus.publish(AppEvent::Error {
            message: "third".into(),
        })
        .await;

        let log = bus.event_log.read().await;
        assert_eq!(log.len(), 2);
        assert!(matches!(log.front(), Some(AppEvent::Error { message }) if message == "second"));
        assert!(matches!(log.back(), Some(AppEvent::Error { message }) if message == "third"));
    }
}

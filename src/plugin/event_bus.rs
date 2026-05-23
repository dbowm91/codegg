use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::RwLock;

use crate::bus::events::AppEvent;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginEventSubscription {
    pub plugin_id: String,
    pub event_patterns: Vec<String>,
    pub priority: i32,
}

pub struct PluginEventBus {
    subscriptions: Arc<RwLock<Vec<PluginEventSubscription>>>,
    event_log: Arc<RwLock<Vec<AppEvent>>>,
    max_log_size: usize,
}

impl PluginEventBus {
    pub fn new(max_log_size: usize) -> Self {
        Self {
            subscriptions: Arc::new(RwLock::new(Vec::new())),
            event_log: Arc::new(RwLock::new(Vec::new())),
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
        {
            let mut log = self.event_log.write().await;
            log.push(event.clone());
            if log.len() > self.max_log_size {
                log.remove(0);
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

    pub async fn get_event_log(&self) -> Vec<AppEvent> {
        self.event_log.read().await.clone()
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

use crate::bus::events::AppEvent;
use std::sync::LazyLock;
use tokio::sync::broadcast;

static GLOBAL_BUS: LazyLock<GlobalEventBus> = LazyLock::new(GlobalEventBus::new);

pub struct GlobalEventBus {
    tx: broadcast::Sender<AppEvent>,
}

impl GlobalEventBus {
    pub fn new() -> Self {
        let (tx, _rx) = broadcast::channel(2048);
        Self { tx }
    }

    pub fn publish(event: AppEvent) {
        if GLOBAL_BUS.tx.send(event.clone()).is_err() {
            tracing::warn!(
                "No subscribers for event: {:?}",
                std::mem::discriminant(&event)
            );
        }
    }

    pub fn subscribe() -> broadcast::Receiver<AppEvent> {
        GLOBAL_BUS.tx.subscribe()
    }

    pub fn subscriber_count() -> usize {
        GLOBAL_BUS.tx.receiver_count()
    }
}

impl Default for GlobalEventBus {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_global_bus_subscribe_count() {
        let count = GlobalEventBus::subscriber_count();
        assert_eq!(count, 0);
    }
}
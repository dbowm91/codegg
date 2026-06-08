use crate::bus::events::AppEvent;
use std::sync::LazyLock;
use tokio::sync::broadcast;

static GLOBAL_BUS: LazyLock<GlobalEventBus> = LazyLock::new(GlobalEventBus::new);

pub struct GlobalEventBus {
    tx: broadcast::Sender<AppEvent>,
}

impl GlobalEventBus {
    pub fn new() -> Self {
        let (tx, _rx) = broadcast::channel(4096);
        Self { tx }
    }

    pub fn publish(event: AppEvent) {
        let discriminant = std::mem::discriminant(&event);
        match GLOBAL_BUS.tx.send(event) {
            Ok(0) => tracing::debug!("No subscribers for event: {:?}", discriminant),
            Ok(n) => tracing::trace!("Event published to {} subscribers: {:?}", n, discriminant),
            Err(e) => tracing::warn!("Failed to publish event (channel closed): {:?}", e),
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

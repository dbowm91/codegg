use crate::bus::events::AppEvent;
use std::sync::LazyLock;
use tokio::sync::broadcast;

static GLOBAL_BUS: LazyLock<GlobalEventBus> = LazyLock::new(GlobalEventBus::new);

/// A global event bus for publish-subscribe messaging across the application.
///
/// All events published through this bus are broadcast to all subscribers.
/// The bus uses a broadcast channel with a buffer of 2048 events.
pub struct GlobalEventBus {
    tx: broadcast::Sender<AppEvent>,
}

impl GlobalEventBus {
    /// Creates a new GlobalEventBus with a broadcast channel.
    pub fn new() -> Self {
        let (tx, _rx) = broadcast::channel(2048);
        Self { tx }
    }

    /// Publishes an event to all subscribers.
    pub fn publish(event: AppEvent) {
        let _ = GLOBAL_BUS.tx.send(event.clone());
        let _ = std::io::Write::write_all(
            &mut std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open("codegg_debug.log")
                .unwrap_or_else(|_| std::fs::File::create("/dev/null").unwrap()),
            format!("[BUS-PUB] {:?}\n", std::mem::discriminant(&event)).as_bytes(),
        );
    }

    /// Creates a new receiver that will receive all subsequent events.
    pub fn subscribe() -> broadcast::Receiver<AppEvent> {
        GLOBAL_BUS.tx.subscribe()
    }

    /// Returns the number of active subscribers.
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

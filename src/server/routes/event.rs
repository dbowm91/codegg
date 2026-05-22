use axum::{
    extract::State,
    response::sse::{Event, Sse},
};
use futures::stream::Stream;
use std::convert::Infallible;
use std::time::Duration;
use tokio_stream::wrappers::BroadcastStream;
use tokio_stream::StreamExt;

use crate::bus::events::AppEvent;

#[derive(Clone)]
pub struct EventBus {
    tx: tokio::sync::broadcast::Sender<String>,
}

impl EventBus {
    pub fn new() -> Self {
        let (tx, _rx) = tokio::sync::broadcast::channel(1024);
        Self { tx }
    }

    pub fn publish(&self, event: &str) {
        if self.tx.send(event.to_string()).is_err() {
            tracing::warn!("EventBus publish failed: no subscribers");
        }
    }

    pub fn subscribe(&self) -> tokio::sync::broadcast::Receiver<String> {
        self.tx.subscribe()
    }
}

impl Default for EventBus {
    fn default() -> Self {
        Self::new()
    }
}

pub async fn sse_handler(
    State(_bus): State<GlobalEventBus>,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    let mut rx = crate::bus::global::GlobalEventBus::subscribe();
    let stream = BroadcastStream::new(rx).filter_map(|result| match result {
        Ok(event) => {
            if let Ok(json) = serde_json::to_string(&event) {
                let line = format!("event: {}\ndata: {}\n\n", event.event_type(), json);
                Some(Ok(Event::default().data(line)))
            } else {
                None
            }
        }
        Err(_) => None,
    });

    let heartbeat =
        tokio_stream::wrappers::IntervalStream::new(tokio::time::interval(Duration::from_secs(15)))
            .map(|_| Ok(Event::default().comment("heartbeat")));

    Sse::new(stream.merge(heartbeat))
        .keep_alive(axum::response::sse::KeepAlive::new().interval(Duration::from_secs(15)))
}

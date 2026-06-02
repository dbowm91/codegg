use axum::{
    response::sse::{Event, Sse},
};
use futures::stream::Stream;
use std::convert::Infallible;
use std::time::Duration;
use tokio_stream::wrappers::BroadcastStream;
use tokio_stream::StreamExt;



pub async fn sse_handler() -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    let rx = crate::bus::global::GlobalEventBus::subscribe();
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

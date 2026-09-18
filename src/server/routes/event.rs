use axum::{
    extract::{Extension, State},
    response::sse::{Event, Sse},
};
use futures_util::stream::Stream;
use std::convert::Infallible;
use std::time::Duration;
use tokio_stream::wrappers::errors::BroadcastStreamRecvError;
use tokio_stream::wrappers::BroadcastStream;
use tokio_stream::StreamExt;

use super::super::authz;
use super::super::state::ServerState;
use crate::error::AxumAppError;
use codegg_core::transport_auth::AuthenticatedPrincipal;

/// Global SSE compatibility stream.
///
/// The global `GlobalEventBus` carries unfiltered cross-project events, so
/// it is LocalOwner-only compatibility. Team principals fail closed with a
/// privacy-safe 404 and never receive a stream; LocalOwner broad policy
/// passes through the canonical authorization service. A future milestone
/// may adapt this to the authorized projection/subscription machinery with
/// explicit scope instead of the global bus.
pub async fn sse_handler(
    Extension(principal): Extension<AuthenticatedPrincipal>,
    State(state): State<ServerState>,
) -> Result<Sse<impl Stream<Item = Result<Event, Infallible>>>, AxumAppError> {
    authz::require_local_owner(&state.pool, &principal, "event_subscribe").await?;
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
        Err(BroadcastStreamRecvError::Lagged(n)) => {
            tracing::warn!(dropped = n, "SSE event subscriber lagged");
            Some(Ok(Event::default()
                .event("resync_required")
                .data(format!("{{\"dropped\":{n}}}"))))
        }
    });

    let heartbeat =
        tokio_stream::wrappers::IntervalStream::new(tokio::time::interval(Duration::from_secs(15)))
            .map(|_| Ok(Event::default().comment("heartbeat")));

    Ok(Sse::new(stream.merge(heartbeat))
        .keep_alive(axum::response::sse::KeepAlive::new().interval(Duration::from_secs(15))))
}

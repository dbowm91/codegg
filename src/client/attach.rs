use futures::{SinkExt, StreamExt};
use std::time::Duration;
use tokio::time::timeout;
use tokio_tungstenite::{connect_async, tungstenite::Message};
use tokio_tungstenite::tungstenite::http::Request;
use tracing::{error, info};

use crate::protocol::tui::TuiMessage;
use crate::client::sdk::RemoteClient;
use crate::error::ClientError;
use crate::tui;

pub async fn run_attach(url: &str, token: Option<&str>) -> Result<(), ClientError> {
    let ws_url = build_tui_ws_url(url);
    let http_url = build_http_url(url);

    let client = RemoteClient::new(&http_url, token)?;

    info!("Connecting to {}", http_url);

    client.health().await?;

    info!("Connected, establishing TUI WebSocket...");

    let mut request = Request::builder().uri(&ws_url);
    if let Some(t) = token {
        request = request.header("Authorization", format!("Bearer {}", t));
    }
    let ws_request = request
        .body(())
        .map_err(|e| ClientError::Connection(format!("invalid WebSocket request: {}", e)))?;

    let ws_stream = match timeout(Duration::from_secs(30), connect_async(ws_request)).await {
        Ok(Ok((stream, _))) => stream,
        Ok(Err(e)) => return Err(ClientError::WebSocket(e.to_string())),
        Err(_) => return Err(ClientError::Connection("WebSocket connection timed out".to_string())),
    };

    info!("TUI WebSocket connected");

    let (mut ws_tx, mut ws_rx) = ws_stream.split();

    let mut app = tui::App::new_remote(url.to_string());

    let (event_tx, event_rx) = tokio::sync::mpsc::unbounded_channel::<serde_json::Value>();
    let (out_tx, mut out_rx) = tokio::sync::mpsc::unbounded_channel::<TuiMessage>();

    app.set_remote_event_rx(event_rx);
    app.set_remote_send_tx(out_tx);

    let event_task = tokio::spawn(async move {
        while let Some(msg) = ws_rx.next().await {
            match msg {
                Ok(Message::Text(text)) => {
                    match serde_json::from_str::<serde_json::Value>(&text) {
                        Ok(event) => {
                            if event_tx.send(event).is_err() {
                                tracing::debug!("event_tx closed, stopping event_task");
                                break;
                            }
                        }
                        Err(e) => {
                            tracing::warn!("failed to parse WebSocket message: {}", e);
                        }
                    }
                }
                Ok(Message::Close(_)) => {
                    info!("Server closed connection");
                    break;
                }
                Err(e) => {
                    error!("WebSocket error: {}", e);
                    break;
                }
                _ => {}
            }
        }
    });

    let send_task = tokio::spawn(async move {
        while let Some(msg) = out_rx.recv().await {
            if let Ok(json) = serde_json::to_string(&msg) {
                if ws_tx.send(Message::Text(json.into())).await.is_err() {
                    break;
                }
            }
        }
    });

    let result = tui::run_event_loop(&mut app).await;

    event_task.abort();
    send_task.abort();

    result.map_err(|e| ClientError::Connection(e.to_string()))
}

fn build_tui_ws_url(url: &str) -> String {
    let base = url.trim_end_matches('/');
    if base.starts_with("wss://") || base.starts_with("ws://") {
        format!("{}/tui", base)
    } else if base.starts_with("https://") {
        format!("{}/tui", base.replace("https://", "wss://"))
    } else if base.starts_with("http://") {
        format!("{}/tui", base.replace("http://", "ws://"))
    } else {
        format!("{}/tui", base)
    }
}

fn build_http_url(url: &str) -> String {
    let base = url.trim_end_matches('/');
    if base.starts_with("ws://") {
        base.replace("ws://", "http://")
    } else if base.starts_with("wss://") {
        base.replace("wss://", "https://")
    } else {
        base.to_string()
    }
}

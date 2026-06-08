use futures::{SinkExt, StreamExt};
use std::time::Duration;
use tokio::time::sleep;
use tokio::time::timeout;
use tokio_tungstenite::tungstenite::http::Request;
use tokio_tungstenite::{connect_async, tungstenite::Message};
use tracing::{error, info, warn};

use crate::client::sdk::RemoteClient;
use crate::error::ClientError;
use crate::protocol::tui::TuiMessage;
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

    let ws_stream = {
        let mut attempt = 0;
        let max_attempts = 3;
        loop {
            if attempt > 0 {
                let delay_secs = 2u64.saturating_pow((attempt - 1) as u32);
                info!("WebSocket reconnect attempt {} in {}s", attempt, delay_secs);
                sleep(Duration::from_secs(delay_secs)).await;
            }
            match timeout(Duration::from_secs(30), connect_async(ws_request.clone())).await {
                Ok(Ok((stream, _))) => break stream,
                Ok(Err(e)) => {
                    if attempt >= max_attempts {
                        return Err(ClientError::WebSocket(format!(
                            "WebSocket connection to {} failed after {} attempts: {}",
                            ws_url, max_attempts, e
                        )));
                    }
                    warn!(
                        "WebSocket attempt {} failed: {}, retrying...",
                        attempt + 1,
                        e
                    );
                }
                Err(_) => {
                    if attempt >= max_attempts {
                        return Err(ClientError::Connection(format!(
                            "WebSocket connection to {} timed out after {} attempts",
                            ws_url, max_attempts
                        )));
                    }
                    warn!("WebSocket attempt {} timed out, retrying...", attempt + 1);
                }
            }
            attempt += 1;
        }
    };

    info!("TUI WebSocket connected");

    let (mut ws_tx, mut ws_rx) = ws_stream.split();

    // Request a resumable stream from sequence 0 (full resync fallback).
    if let Ok(resume) = serde_json::to_string(&TuiMessage::Resume { from_event_seq: 0 }) {
        let _ = ws_tx.send(Message::Text(resume.into())).await;
    }

    let mut app = tui::App::new_remote(url.to_string());

    let (event_tx, event_rx) = tokio::sync::mpsc::unbounded_channel::<serde_json::Value>();
    let (out_tx, mut out_rx) = tokio::sync::mpsc::unbounded_channel::<TuiMessage>();

    app.set_remote_event_rx(event_rx);
    app.set_remote_send_tx(out_tx);

    let event_task = tokio::spawn(async move {
        while let Some(msg) = ws_rx.next().await {
            match msg {
                Ok(Message::Text(text)) => match serde_json::from_str::<serde_json::Value>(&text) {
                    Ok(event) => {
                        if event_tx.send(event).is_err() {
                            tracing::debug!("event_tx closed, stopping event_task");
                            break;
                        }
                    }
                    Err(e) => {
                        tracing::warn!("failed to parse WebSocket message: {}", e);
                    }
                },
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

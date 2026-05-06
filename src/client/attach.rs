use futures::{SinkExt, StreamExt};
use std::sync::Arc;
use tokio::sync::Mutex;
use tokio_tungstenite::{connect_async, tungstenite::Message};
use tracing::{error, info};

use crate::client::sdk::RemoteClient;
use crate::error::ClientError;
use crate::tui;

#[derive(Debug, serde::Serialize, serde::Deserialize, Clone)]
#[serde(tag = "type")]
pub enum TuiMessage {
    Input {
        text: String,
    },
    KeyDown {
        key: String,
        modifiers: Vec<String>,
    },
    MouseClick {
        x: u16,
        y: u16,
    },
    Resize {
        w: u16,
        h: u16,
    },
    PermissionResponse {
        id: String,
        choice: String,
    },
    QuestionResponse {
        id: String,
        answers: Vec<String>,
    },
    TextDelta {
        delta: String,
    },
    PermissionPending {
        id: String,
        tool: String,
        path: Option<String>,
    },
    QuestionPending {
        id: String,
        questions: Vec<QuestionSpec>,
    },
    SessionInfo {
        id: String,
        model: String,
    },
    SessionEnded {
        stop_reason: String,
    },
    ToolCallStarted {
        tool_name: String,
        tool_id: String,
        arguments: String,
    },
    ToolResult {
        tool_id: String,
        output: String,
        success: bool,
    },
}

#[derive(Debug, serde::Serialize, serde::Deserialize, Clone)]
pub struct QuestionSpec {
    pub id: String,
    pub prompt: String,
    pub default: Option<String>,
}

pub async fn run_attach(url: &str, token: Option<&str>) -> Result<(), ClientError> {
    let ws_url = build_tui_ws_url(url);
    let http_url = build_http_url(url);

    let client = RemoteClient::new(&http_url, token)?;

    info!("Connecting to {}", http_url);

    client.health().await?;

    info!("Connected, establishing TUI WebSocket...");

    let (ws_stream, _) = connect_async(&ws_url)
        .await
        .map_err(|e| ClientError::WebSocket(e.to_string()))?;

    info!("TUI WebSocket connected");

    let (tx, mut rx) = ws_stream.split();

    let mut app = tui::App::new_remote(url.to_string());

    let (event_tx, event_rx) = tokio::sync::mpsc::unbounded_channel::<serde_json::Value>();
    let (_, input_rx) = tokio::sync::mpsc::unbounded_channel::<String>();

    app.set_remote_event_rx(event_rx);

    let tx = Arc::new(Mutex::new(tx));
    let tx_clone = Arc::clone(&tx);

    let input_handler = tokio::spawn(async move {
        let tx = tx_clone;
        let mut input_rx = input_rx;
        while let Some(text) = input_rx.recv().await {
            let msg = TuiMessage::Input { text };
            if let Ok(json) = serde_json::to_string(&msg) {
                let mut tx = tx.lock().await;
                let _ = tx.send(Message::Text(json.into())).await;
            }
        }
    });

    let event_task = tokio::spawn(async move {
        while let Some(msg) = rx.next().await {
            match msg {
                Ok(Message::Text(text)) => {
                    if let Ok(event) = serde_json::from_str::<serde_json::Value>(&text) {
                        let _ = event_tx.send(event);
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

    let result = tui::run_event_loop(&mut app).await;

    input_handler.abort();
    event_task.abort();

    let mut tx = tx.lock().await;
    let _ = tx.close().await;

    result.map_err(|e| ClientError::Connection(e.to_string()))
}

fn build_tui_ws_url(url: &str) -> String {
    let base = url.trim_end_matches('/');
    if base.starts_with("wss://") || base.starts_with("ws://") {
        format!("{}/tui", base.replace("ws://", "wss://"))
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

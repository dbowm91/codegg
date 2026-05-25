use async_trait::async_trait;
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixStream;
use tokio::sync::{mpsc, Mutex};

use crate::core::CoreClient;
use crate::error::AppError;
use crate::protocol::core::{CoreEvent, CoreRequest, CoreResponse, EventEnvelope, RequestEnvelope};

#[derive(Clone)]
pub struct SocketCoreClient {
    endpoint: String,
    stream: Arc<Mutex<Option<UnixStream>>>,
}

impl SocketCoreClient {
    pub async fn connect(endpoint: &str) -> Result<Self, AppError> {
        let path = endpoint.strip_prefix("unix://").unwrap_or(endpoint);
        let stream = UnixStream::connect(path).await.map_err(|e| {
            AppError::Other(anyhow::anyhow!("failed to connect socket core '{}': {}", path, e))
        })?;
        Ok(Self {
            endpoint: endpoint.to_string(),
            stream: Arc::new(Mutex::new(Some(stream))),
        })
    }

    async fn reconnect(&self) -> Result<(), AppError> {
        let path = self.endpoint.strip_prefix("unix://").unwrap_or(&self.endpoint);
        let stream = UnixStream::connect(path).await.map_err(|e| {
            AppError::Other(anyhow::anyhow!(
                "failed to reconnect socket core '{}': {}",
                path,
                e
            ))
        })?;
        *self.stream.lock().await = Some(stream);
        Ok(())
    }
}

#[async_trait]
impl CoreClient for SocketCoreClient {
    async fn request(
        &self,
        request: RequestEnvelope<CoreRequest>,
    ) -> Result<CoreResponse, AppError> {
        // Attempt once, then reconnect and retry once.
        for attempt in 0..2 {
            let mut guard = self.stream.lock().await;
            if guard.is_none() {
                drop(guard);
                self.reconnect().await?;
                guard = self.stream.lock().await;
            }
            let mut stream = guard.take().ok_or_else(|| {
                AppError::Other(anyhow::anyhow!("socket core stream unavailable"))
            })?;
            drop(guard);

            let payload = serde_json::to_string(&request).map_err(AppError::Json)?;
            if let Err(e) = stream.write_all(payload.as_bytes()).await {
                if attempt == 0 {
                    self.reconnect().await?;
                    continue;
                }
                return Err(AppError::Other(anyhow::anyhow!(
                    "socket write failed: {}",
                    e
                )));
            }
            if let Err(e) = stream.write_all(b"\n").await {
                if attempt == 0 {
                    self.reconnect().await?;
                    continue;
                }
                return Err(AppError::Other(anyhow::anyhow!(
                    "socket write failed: {}",
                    e
                )));
            }
            if let Err(e) = stream.flush().await {
                if attempt == 0 {
                    self.reconnect().await?;
                    continue;
                }
                return Err(AppError::Other(anyhow::anyhow!(
                    "socket flush failed: {}",
                    e
                )));
            }

            let mut reader = BufReader::new(stream);
            let mut line = String::new();
            if let Err(e) = reader.read_line(&mut line).await {
                if attempt == 0 {
                    self.reconnect().await?;
                    continue;
                }
                return Err(AppError::Other(anyhow::anyhow!(
                    "socket read failed: {}",
                    e
                )));
            }
            let inner = reader.into_inner();
            *self.stream.lock().await = Some(inner);

            if line.trim().is_empty() {
                if attempt == 0 {
                    self.reconnect().await?;
                    continue;
                }
                return Err(AppError::Other(anyhow::anyhow!(
                    "empty response from socket core"
                )));
            }
            return serde_json::from_str::<CoreResponse>(line.trim()).map_err(AppError::Json);
        }

        Err(AppError::Other(anyhow::anyhow!(
            "socket core request failed after reconnect attempt"
        )))
    }

    fn subscribe(&self) -> mpsc::UnboundedReceiver<EventEnvelope<CoreEvent>> {
        let (_tx, rx) = mpsc::unbounded_channel();
        rx
    }
}

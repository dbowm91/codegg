use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use dashmap::DashMap;
use once_cell::sync::Lazy;
use tokio::sync::mpsc;

static PLAN_REGISTRY: Lazy<PlanRegistry> = Lazy::new(PlanRegistry::new);

pub struct PlanRegistry {
    pending: Arc<tokio::sync::Mutex<HashMap<String, PlanRequest>>>,
    response_txs: DashMap<String, mpsc::Sender<PlanResponse>>,
}

pub struct PlanRequest {
    pub session_id: String,
    pub plan_description: String,
    pub created_at: Instant,
}

#[derive(Debug, Clone)]
pub enum PlanResponse {
    Confirmed,
    Cancelled,
}

impl PlanRegistry {
    pub fn new() -> Self {
        Self {
            pending: Arc::new(tokio::sync::Mutex::new(HashMap::new())),
            response_txs: DashMap::new(),
        }
    }

    pub async fn register(&self, request: PlanRequest) -> String {
        let id = uuid::Uuid::new_v4().to_string();
        self.pending.lock().await.insert(id.clone(), request);
        id
    }

    pub async fn register_with_sender(
        &self,
        id: String,
        tx: mpsc::Sender<PlanResponse>,
    ) {
        self.pending.lock().await.insert(id.clone(), PlanRequest {
            session_id: String::new(),
            plan_description: String::new(),
            created_at: Instant::now(),
        });
        self.response_txs.insert(id, tx);
    }

    pub async fn respond(&self, id: &str, response: PlanResponse) -> bool {
        if let Some((_, tx)) = self.response_txs.remove(id) {
            tx.send(response).await.is_ok()
        } else {
            false
        }
    }

    pub async fn unregister(&self, id: &str) {
        self.pending.lock().await.remove(id);
        self.response_txs.remove(id);
    }
}

impl Default for PlanRegistry {
    fn default() -> Self {
        Self::new()
    }
}

pub async fn wait_for_response(
    id: &str,
    timeout: Duration,
) -> Result<PlanResponse, PlanError> {
    let registry = registry();
    let tx = match registry.response_txs.get(id) {
        Some(entry) => entry.value().clone(),
        None => return Err(PlanError::NotFound),
    };

    let (response_tx, mut response_rx) = mpsc::channel(1);
    let _ = tx.send(PlanResponse::Cancelled).await;
    let _ = response_tx;

    tokio::select! {
        result = response_rx.recv() => {
            match result {
                Some(response) => Ok(response),
                None => Err(PlanError::ChannelClosed),
            }
        }
        _ = tokio::time::sleep(timeout) => Err(PlanError::Timeout),
    }
}

#[derive(Debug)]
pub enum PlanError {
    Timeout,
    NotFound,
    UnexpectedResponse,
    ChannelClosed,
}

impl std::fmt::Display for PlanError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PlanError::Timeout => write!(f, "timeout waiting for plan response"),
            PlanError::NotFound => write!(f, "plan not found"),
            PlanError::UnexpectedResponse => write!(f, "unexpected response"),
            PlanError::ChannelClosed => write!(f, "channel closed"),
        }
    }
}

impl std::error::Error for PlanError {}

pub fn registry() -> &'static PlanRegistry {
    &PLAN_REGISTRY
}
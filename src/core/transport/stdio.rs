use async_trait::async_trait;
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{ChildStdin, ChildStdout, Command};
use tokio::sync::{mpsc, Mutex};

use crate::core::CoreClient;
use crate::error::AppError;
use crate::protocol::core::{CoreEvent, CoreRequest, CoreResponse, EventEnvelope, RequestEnvelope};

#[derive(Clone)]
pub struct StdioCoreClient {
    stdin: Arc<Mutex<ChildStdin>>,
    stdout: Arc<Mutex<BufReader<ChildStdout>>>,
}

impl StdioCoreClient {
    pub async fn spawn(
        program: &str,
        args: &[String],
        cwd: Option<&std::path::Path>,
    ) -> Result<Self, AppError> {
        let mut cmd = Command::new(program);
        cmd.args(args)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::inherit());
        if let Some(cwd) = cwd {
            cmd.current_dir(cwd);
        }
        let mut child = cmd
            .spawn()
            .map_err(|e| AppError::Other(anyhow::anyhow!("failed to spawn stdio core: {}", e)))?;
        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| AppError::Other(anyhow::anyhow!("missing child stdin")))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| AppError::Other(anyhow::anyhow!("missing child stdout")))?;
        Ok(Self {
            stdin: Arc::new(Mutex::new(stdin)),
            stdout: Arc::new(Mutex::new(BufReader::new(stdout))),
        })
    }
}

#[async_trait]
impl CoreClient for StdioCoreClient {
    async fn request(
        &self,
        request: RequestEnvelope<CoreRequest>,
    ) -> Result<CoreResponse, AppError> {
        let payload = serde_json::to_string(&request).map_err(AppError::Json)?;
        {
            let mut stdin = self.stdin.lock().await;
            stdin
                .write_all(payload.as_bytes())
                .await
                .map_err(|e| AppError::Other(anyhow::anyhow!("stdio write failed: {}", e)))?;
            stdin
                .write_all(b"\n")
                .await
                .map_err(|e| AppError::Other(anyhow::anyhow!("stdio write failed: {}", e)))?;
            stdin
                .flush()
                .await
                .map_err(|e| AppError::Other(anyhow::anyhow!("stdio flush failed: {}", e)))?;
        }

        let mut line = String::new();
        {
            let mut stdout = self.stdout.lock().await;
            stdout
                .read_line(&mut line)
                .await
                .map_err(|e| AppError::Other(anyhow::anyhow!("stdio read failed: {}", e)))?;
        }
        if line.trim().is_empty() {
            return Err(AppError::Other(anyhow::anyhow!(
                "empty response from stdio core"
            )));
        }
        serde_json::from_str::<CoreResponse>(line.trim()).map_err(AppError::Json)
    }

    fn subscribe(&self) -> mpsc::UnboundedReceiver<EventEnvelope<CoreEvent>> {
        let (_tx, rx) = mpsc::unbounded_channel();
        rx
    }
}

//! LSP Launch - spawns and manages language server processes.
//!
//! Handles low-level process management:
//! - Spawns servers with proper stdin/stdout/stderr
//! - Sends JSON-RPC requests via Content-Length framing
//! - Background stderr drain (capped at 64KB)
//! - Provides graceful termination via kill()
//!
//! stdout is exclusively owned by the background reader task in `client.rs`;
//! this module does not read responses or notifications.

use std::path::Path;
use std::process::Stdio;

use tokio::io::{AsyncWriteExt, BufReader};
use tokio::process::Command;
use tracing::{debug, error, info};

use crate::error::LspError;

pub struct LspProcess {
    pub stdin: tokio::process::ChildStdin,
    pub stdout: Option<tokio::process::ChildStdout>,
    pub stderr: Option<BufReader<tokio::process::ChildStderr>>,
    pub child: tokio::process::Child,
}

pub async fn spawn_server(
    command: &str,
    args: &[&str],
    env: &[(String, String)],
    cwd: Option<&Path>,
) -> Result<LspProcess, LspError> {
    let mut cmd = Command::new(command);
    cmd.args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .env_clear();

    if let Some(user_path) = std::env::var_os("PATH") {
        cmd.env("PATH", user_path);
    } else {
        cmd.env("PATH", "/usr/local/bin:/usr/bin:/bin");
    }

    for (k, v) in env {
        cmd.env(k, v);
    }

    if let Some(dir) = cwd {
        cmd.current_dir(dir);
    }

    let mut child = cmd.spawn().map_err(|e| {
        LspError::LaunchFailed(format!("failed to spawn '{} {:?}': {}", command, args, e))
    })?;

    let stdin = child
        .stdin
        .take()
        .ok_or_else(|| LspError::LaunchFailed("failed to capture stdin".to_string()))?;

    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| LspError::LaunchFailed("failed to capture stdout".to_string()))?;

    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| LspError::LaunchFailed("failed to capture stderr".to_string()))?;

    let stderr_reader = BufReader::new(stderr);

    info!(command, args = ?args, "spawned LSP server");

    Ok(LspProcess {
        stdin,
        stdout: Some(stdout),
        stderr: Some(stderr_reader),
        child,
    })
}

pub async fn send_request(process: &mut LspProcess, msg: &str) -> Result<(), LspError> {
    let content = format!("Content-Length: {}\r\n\r\n{}", msg.len(), msg);
    process
        .stdin
        .write_all(content.as_bytes())
        .await
        .map_err(|e| LspError::RequestFailed(format!("write failed: {}", e)))?;
    process
        .stdin
        .flush()
        .await
        .map_err(|e| LspError::RequestFailed(format!("flush failed: {}", e)))?;
    debug!(msg_len = msg.len(), "sent LSP request");
    Ok(())
}

pub fn spawn_stderr_drain(server_id: &str, stderr: tokio::process::ChildStderr) {
    let server_id = server_id.to_string();
    tokio::spawn(async move {
        use tokio::io::AsyncReadExt;
        let mut reader = BufReader::new(stderr);
        let mut buf = vec![0u8; 8192];
        let mut total_bytes: usize = 0;
        const MAX_STDERR_BYTES: usize = 64 * 1024;
        loop {
            match reader.read(&mut buf).await {
                Ok(0) => break,
                Ok(n) => {
                    total_bytes += n;
                    if total_bytes <= MAX_STDERR_BYTES {
                        let chunk = String::from_utf8_lossy(&buf[..n]);
                        debug!(server = %server_id, "LSP stderr: {}", chunk.trim());
                    }
                }
                Err(_) => break,
            }
        }
    });
}

pub async fn terminate(process: &mut LspProcess) {
    if let Err(e) = process.child.kill().await {
        error!(error = %e, "failed to kill LSP process");
    }
}

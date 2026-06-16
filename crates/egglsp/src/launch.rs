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

use std::path::{Path, PathBuf};
use std::process::Stdio;

use tokio::io::{AsyncWriteExt, BufReader};
use tokio::process::Command;
use tracing::{debug, error, info};

use crate::error::LspError;

/// Owned launch description for a single child process.
///
/// The production runtime resolves static registry entries or config
/// overrides into this owned representation before spawning.
#[derive(Debug, Clone)]
pub struct LspLaunchSpec {
    pub id: String,
    pub command: PathBuf,
    pub args: Vec<String>,
    pub env: Vec<(String, String)>,
    pub languages: Vec<String>,
    pub extensions: Vec<String>,
}

impl LspLaunchSpec {
    pub fn new(
        id: impl Into<String>,
        command: impl Into<PathBuf>,
        args: Vec<String>,
        env: Vec<(String, String)>,
        languages: Vec<String>,
        extensions: Vec<String>,
    ) -> Self {
        Self {
            id: id.into(),
            command: command.into(),
            args,
            env,
            languages,
            extensions,
        }
    }

    /// Default for test fixtures. Empty command; downstream code
    /// should never spawn it. Available unconditionally because
    /// the init pipeline's `Option<LspClientDescriptor>` path uses
    /// it as a placeholder when the test factory doesn't provide
    /// a real launch spec.
    pub fn default_for_test() -> Self {
        Self {
            id: String::new(),
            command: PathBuf::new(),
            args: Vec::new(),
            env: Vec::new(),
            languages: Vec::new(),
            extensions: Vec::new(),
        }
    }
}

pub struct LspProcess {
    pub stdin: Option<tokio::process::ChildStdin>,
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
    let command = PathBuf::from(command);
    let args: Vec<String> = args.iter().map(|arg| (*arg).to_string()).collect();
    spawn_server_impl(&command, &args, env, cwd, true).await
}

/// Spawns from a resolved command path plus owned args/env.
///
/// This path does not consult the parent process PATH; callers must
/// pass a resolved executable path.
pub async fn spawn_server_owned(
    command: &Path,
    args: &[String],
    env: &[(String, String)],
    cwd: Option<&Path>,
) -> Result<LspProcess, LspError> {
    spawn_server_impl(command, args, env, cwd, false).await
}

async fn spawn_server_impl(
    command: &Path,
    args: &[String],
    env: &[(String, String)],
    cwd: Option<&Path>,
    include_parent_path: bool,
) -> Result<LspProcess, LspError> {
    let mut cmd = Command::new(command);
    cmd.args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .env_clear();

    if include_parent_path {
        if let Some(user_path) = std::env::var_os("PATH") {
            cmd.env("PATH", user_path);
        } else {
            cmd.env("PATH", "/usr/local/bin:/usr/bin:/bin");
        }
    }

    cmd.envs(env.iter().map(|(k, v)| (k, v)));

    if let Some(dir) = cwd {
        cmd.current_dir(dir);
    }

    let mut child = cmd.spawn().map_err(|e| {
        LspError::LaunchFailed(format!(
            "failed to spawn '{} {:?}': {}",
            command.display(),
            args,
            e
        ))
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

    info!(command = %command.display(), args = ?args, "spawned LSP server");

    Ok(LspProcess {
        stdin: Some(stdin),
        stdout: Some(stdout),
        stderr: Some(stderr_reader),
        child,
    })
}

pub async fn send_request(process: &mut LspProcess, msg: &str) -> Result<(), LspError> {
    let stdin = process
        .stdin
        .as_mut()
        .ok_or_else(|| LspError::RequestFailed("stdin not available".to_string()))?;
    let content = format!("Content-Length: {}\r\n\r\n{}", msg.len(), msg);
    stdin
        .write_all(content.as_bytes())
        .await
        .map_err(|e| LspError::RequestFailed(format!("write failed: {}", e)))?;
    stdin
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

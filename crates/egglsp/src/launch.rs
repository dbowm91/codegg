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

use async_trait::async_trait;
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
    /// Private helper status monitor retained by contained launchers until
    /// the authoritative process runtime has reaped the child.
    pub status_task: Option<tokio::task::JoinHandle<Result<(), String>>>,
}

/// Process owner used by an `LspService` for every initial launch and restart.
///
/// Embedders can provide a daemon-owned launcher that applies execution policy
/// while preserving the LSP's supervised stdin/stdout/stderr pipes. The
/// default implementation retains egglsp's historical direct-spawn behavior.
#[allow(clippy::double_must_use)]
#[async_trait]
pub trait LspProcessLauncher: Send + Sync {
    async fn launch(&self, spec: &LspLaunchSpec, root: &Path) -> Result<LspProcess, LspError>;
}

pub struct BoundedStartLspProcessLauncher {
    inner: std::sync::Arc<dyn LspProcessLauncher>,
    active: std::sync::Arc<tokio::sync::Semaphore>,
    admitted: std::sync::Arc<tokio::sync::Semaphore>,
}

impl BoundedStartLspProcessLauncher {
    pub fn new(
        inner: std::sync::Arc<dyn LspProcessLauncher>,
        max_concurrent_starts: usize,
        max_admitted_starts: usize,
    ) -> Result<Self, LspError> {
        if max_concurrent_starts == 0 || max_admitted_starts < max_concurrent_starts {
            return Err(LspError::InvalidConfig(
                "LSP start admission requires max_admitted_starts >= max_concurrent_starts > 0"
                    .to_string(),
            ));
        }
        Ok(Self {
            inner,
            active: std::sync::Arc::new(tokio::sync::Semaphore::new(max_concurrent_starts)),
            admitted: std::sync::Arc::new(tokio::sync::Semaphore::new(max_admitted_starts)),
        })
    }
}

#[async_trait]
impl LspProcessLauncher for BoundedStartLspProcessLauncher {
    async fn launch(&self, spec: &LspLaunchSpec, root: &Path) -> Result<LspProcess, LspError> {
        let _admitted =
            self.admitted.clone().try_acquire_owned().map_err(|_| {
                LspError::RequestFailed("LSP start queue is at capacity".to_string())
            })?;
        let _active =
            self.active.clone().acquire_owned().await.map_err(|_| {
                LspError::InitializationCancelled("LSP start admission closed".into())
            })?;
        self.inner.launch(spec, root).await
    }
}

/// Historical direct launcher used by existing FullHost services.
#[derive(Debug, Default)]
pub struct DirectLspProcessLauncher;

#[async_trait]
impl LspProcessLauncher for DirectLspProcessLauncher {
    async fn launch(&self, spec: &LspLaunchSpec, root: &Path) -> Result<LspProcess, LspError> {
        spawn_server_owned(&spec.command, &spec.args, &spec.env, Some(root)).await
    }
}

#[cfg(test)]
mod admission_tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    struct WaitingLauncher {
        starts: AtomicUsize,
        release: tokio::sync::Notify,
    }

    #[async_trait]
    impl LspProcessLauncher for WaitingLauncher {
        async fn launch(
            &self,
            _spec: &LspLaunchSpec,
            _root: &Path,
        ) -> Result<LspProcess, LspError> {
            self.starts.fetch_add(1, Ordering::SeqCst);
            self.release.notified().await;
            Err(LspError::RequestFailed("test launcher released".into()))
        }
    }

    #[tokio::test]
    async fn bounded_start_admission_rejects_when_the_admitted_queue_is_full() {
        let inner = std::sync::Arc::new(WaitingLauncher {
            starts: AtomicUsize::new(0),
            release: tokio::sync::Notify::new(),
        });
        let launcher = std::sync::Arc::new(
            BoundedStartLspProcessLauncher::new(inner.clone(), 1, 1)
                .expect("valid bounded admission"),
        );
        let spec = LspLaunchSpec::default_for_test();
        let root = std::path::PathBuf::from("/workspace");
        let first = {
            let launcher = launcher.clone();
            let spec = spec.clone();
            let root = root.clone();
            tokio::spawn(async move { launcher.launch(&spec, &root).await })
        };
        while inner.starts.load(Ordering::SeqCst) == 0 {
            tokio::task::yield_now().await;
        }

        let rejected = launcher.launch(&spec, &root).await;
        assert!(
            matches!(rejected, Err(LspError::RequestFailed(message)) if message.contains("queue is at capacity"))
        );
        assert_eq!(inner.starts.load(Ordering::SeqCst), 1);

        first.abort();
        let _ = first.await;
        inner.release.notify_one();
        let retry = launcher.launch(&spec, &root).await;
        assert!(
            matches!(retry, Err(LspError::RequestFailed(message)) if message == "test launcher released")
        );
        assert_eq!(inner.starts.load(Ordering::SeqCst), 2);
    }

    #[test]
    fn bounded_start_admission_rejects_invalid_limits() {
        let inner: std::sync::Arc<dyn LspProcessLauncher> =
            std::sync::Arc::new(DirectLspProcessLauncher);
        assert!(BoundedStartLspProcessLauncher::new(inner.clone(), 0, 1).is_err());
        assert!(BoundedStartLspProcessLauncher::new(inner, 2, 1).is_err());
    }
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
        status_task: None,
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

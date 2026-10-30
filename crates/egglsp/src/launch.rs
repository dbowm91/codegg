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
use tokio_util::sync::CancellationToken;
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
    pub live_process_permit: Option<LspLiveProcessPermit>,
}

#[derive(Debug, Clone)]
pub struct LspLiveProcessBudget {
    permits: std::sync::Arc<tokio::sync::Semaphore>,
    limit: usize,
}

impl LspLiveProcessBudget {
    pub fn new(limit: usize) -> Result<Self, LspError> {
        if limit == 0 {
            return Err(LspError::InvalidConfig(
                "max_live_servers must be greater than zero".into(),
            ));
        }
        Ok(Self {
            permits: std::sync::Arc::new(tokio::sync::Semaphore::new(limit)),
            limit,
        })
    }

    pub fn limit(&self) -> usize {
        self.limit
    }

    pub fn live_processes(&self) -> usize {
        self.limit - self.permits.available_permits()
    }

    fn try_acquire(&self) -> Result<tokio::sync::OwnedSemaphorePermit, LspError> {
        self.permits
            .clone()
            .try_acquire_owned()
            .map_err(|_| LspError::LiveProcessBudgetExhausted { limit: self.limit })
    }
}

#[derive(Debug)]
pub struct LspLiveProcessPermit {
    _permit: Option<tokio::sync::OwnedSemaphorePermit>,
}

impl LspLiveProcessPermit {
    fn new(permit: tokio::sync::OwnedSemaphorePermit) -> Self {
        Self {
            _permit: Some(permit),
        }
    }

    pub(crate) fn release_after_reap(mut self) {
        drop(self._permit.take());
    }

    pub(crate) fn quarantine(mut self) {
        if let Some(permit) = self._permit.take() {
            std::mem::forget(permit);
        }
    }
}

impl Drop for LspLiveProcessPermit {
    fn drop(&mut self) {
        if let Some(permit) = self._permit.take() {
            std::mem::forget(permit);
        }
    }
}

pub struct BudgetedLspProcessLauncher {
    inner: std::sync::Arc<dyn LspProcessLauncher>,
    budget: std::sync::Arc<LspLiveProcessBudget>,
}

impl BudgetedLspProcessLauncher {
    pub fn new(
        inner: std::sync::Arc<dyn LspProcessLauncher>,
        budget: std::sync::Arc<LspLiveProcessBudget>,
    ) -> Self {
        Self { inner, budget }
    }
}

#[async_trait]
impl LspProcessLauncher for BudgetedLspProcessLauncher {
    async fn launch(&self, spec: &LspLaunchSpec, root: &Path) -> Result<LspProcess, LspError> {
        let (mut result_tx, result_rx) = tokio::sync::oneshot::channel();
        let inner = self.inner.clone();
        let budget = self.budget.clone();
        let spec = spec.clone();
        let root = root.to_path_buf();

        tokio::spawn(async move {
            let admission = match budget.try_acquire() {
                Ok(admission) => admission,
                Err(error) => {
                    let _ = result_tx.send(Err(error));
                    return;
                }
            };
            let cancellation = CancellationToken::new();
            let launch = inner.launch_cancellable(&spec, &root, cancellation.clone());
            tokio::pin!(launch);
            let launch_result = tokio::select! {
                result = &mut launch => result,
                _ = result_tx.closed() => {
                    cancellation.cancel();
                    launch.await
                }
            };
            let mut process = match launch_result {
                Ok(process) => process,
                Err(error) => {
                    let _ = result_tx.send(Err(error));
                    return;
                }
            };
            let permit = LspLiveProcessPermit::new(admission);
            process.live_process_permit = Some(permit);
            if process.stdin.is_none() || process.stdout.is_none() || process.stderr.is_none() {
                let _ = result_tx.send(Err(LspError::LaunchFailed(
                    "LSP launcher returned an incomplete process pipe set".into(),
                )));
                reap_unclaimed_process(process).await;
                return;
            }
            if let Err(Ok(process)) = result_tx.send(Ok(process)) {
                reap_unclaimed_process(process).await;
            }
        });

        result_rx.await.unwrap_or_else(|_| {
            Err(LspError::InitializationCancelled(
                "LSP process launch owner stopped before handoff".into(),
            ))
        })
    }
}

pub(crate) async fn reap_unclaimed_process(mut process: LspProcess) {
    let permit = process.live_process_permit.take();
    drop(process.stdin.take());
    drop(process.stdout.take());
    drop(process.stderr.take());
    let _ = process.child.start_kill();
    let reaped = process.child.wait().await.is_ok();
    if let Some(mut status_task) = process.status_task.take() {
        if tokio::time::timeout(std::time::Duration::from_secs(2), &mut status_task)
            .await
            .is_err()
        {
            status_task.abort();
        }
    }
    if let Some(permit) = permit {
        if reaped {
            permit.release_after_reap();
        } else {
            permit.quarantine();
        }
    }
}

/// Per-service launch/restart owner; implementations must reap on cancellation.
#[allow(clippy::double_must_use)]
#[async_trait]
pub trait LspProcessLauncher: Send + Sync {
    async fn launch(&self, spec: &LspLaunchSpec, root: &Path) -> Result<LspProcess, LspError>;

    /// Cooperative cancellation; post-spawn waits must kill and reap.
    async fn launch_cancellable(
        &self,
        spec: &LspLaunchSpec,
        root: &Path,
        cancellation: CancellationToken,
    ) -> Result<LspProcess, LspError> {
        tokio::select! {
            biased;
            _ = cancellation.cancelled() => Err(LspError::InitializationCancelled(
                "LSP process launch was cancelled".into(),
            )),
            result = self.launch(spec, root) => result,
        }
    }
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
        live_process_permit: None,
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

#[cfg(test)]
mod admission_tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    fn sleep_spec(command: &str) -> LspLaunchSpec {
        LspLaunchSpec::new(
            "test-lsp",
            command,
            vec!["-c".into(), "sleep 60".into()],
            Vec::new(),
            Vec::new(),
            Vec::new(),
        )
    }

    async fn into_runtime(
        mut process: LspProcess,
        generation: u64,
    ) -> crate::runtime::LspProcessRuntime {
        let stderr = process.stderr.take().expect("captured stderr").into_inner();
        let permit = process.live_process_permit.take();
        drop(process.stdin.take());
        drop(process.stdout.take());
        let (runtime, _join) = crate::runtime::spawn_process_runtime_with_permit(
            "test-lsp".into(),
            std::env::current_dir().expect("current test directory"),
            generation,
            process.child,
            stderr,
            process.status_task,
            permit,
        );
        runtime
    }

    async fn stop(runtime: &crate::runtime::LspProcessRuntime) {
        runtime.request_force_kill();
        tokio::time::timeout(std::time::Duration::from_secs(5), runtime.wait_for_exit())
            .await
            .expect("child reaped within bound")
            .expect("exit event published");
    }

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

    #[cfg(unix)]
    #[tokio::test]
    async fn daemon_live_budget_competes_across_fullhost_and_constrained_launchers() {
        let budget = std::sync::Arc::new(LspLiveProcessBudget::new(1).expect("budget"));
        let fullhost: std::sync::Arc<dyn LspProcessLauncher> =
            std::sync::Arc::new(BudgetedLspProcessLauncher::new(
                std::sync::Arc::new(DirectLspProcessLauncher),
                budget.clone(),
            ));
        let constrained: std::sync::Arc<dyn LspProcessLauncher> =
            std::sync::Arc::new(BudgetedLspProcessLauncher::new(
                std::sync::Arc::new(DirectLspProcessLauncher),
                budget.clone(),
            ));
        let root = std::env::current_dir().expect("current test directory");
        let spec = sleep_spec("/bin/sh");
        let (fullhost_result, constrained_result) = tokio::join!(
            fullhost.launch(&spec, &root),
            constrained.launch(&spec, &root),
        );
        let (process, rejected) = match (fullhost_result, constrained_result) {
            (Ok(process), Err(error)) | (Err(error), Ok(process)) => (process, error),
            (Ok(left), Ok(right)) => {
                let left = into_runtime(left, 1).await;
                let right = into_runtime(right, 2).await;
                stop(&left).await;
                stop(&right).await;
                panic!("both scopes exceeded one live-server permit");
            }
            (Err(left), Err(right)) => panic!("both launchers refused: {left:?}; {right:?}"),
        };
        assert!(matches!(
            rejected,
            LspError::LiveProcessBudgetExhausted { limit: 1 }
        ));
        assert_eq!(budget.live_processes(), 1);
        let runtime = into_runtime(process, 1).await;
        stop(&runtime).await;
        assert_eq!(budget.live_processes(), 0);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn overlapping_restart_generations_each_consume_and_release_a_permit() {
        let budget = std::sync::Arc::new(LspLiveProcessBudget::new(2).expect("budget"));
        let launcher = BudgetedLspProcessLauncher::new(
            std::sync::Arc::new(DirectLspProcessLauncher),
            budget.clone(),
        );
        let root = std::env::current_dir().expect("current test directory");
        let spec = sleep_spec("/bin/sh");
        let old = into_runtime(launcher.launch(&spec, &root).await.expect("old child"), 1).await;
        let new = into_runtime(
            launcher.launch(&spec, &root).await.expect("restart child"),
            2,
        )
        .await;
        assert_eq!(budget.live_processes(), 2);
        assert!(matches!(
            launcher.launch(&spec, &root).await,
            Err(LspError::LiveProcessBudgetExhausted { limit: 2 })
        ));

        stop(&old).await;
        assert_eq!(budget.live_processes(), 1);
        stop(&new).await;
        assert_eq!(budget.live_processes(), 0);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn crashed_child_releases_its_permit_only_after_wait_reaps_it() {
        let budget = std::sync::Arc::new(LspLiveProcessBudget::new(1).expect("budget"));
        let launcher = BudgetedLspProcessLauncher::new(
            std::sync::Arc::new(DirectLspProcessLauncher),
            budget.clone(),
        );
        let root = std::env::current_dir().expect("current test directory");
        let spec = LspLaunchSpec::new(
            "test-lsp",
            "/bin/sh",
            vec!["-c".into(), "exit 17".into()],
            Vec::new(),
            Vec::new(),
            Vec::new(),
        );
        let runtime = into_runtime(launcher.launch(&spec, &root).await.expect("child"), 1).await;
        tokio::time::timeout(std::time::Duration::from_secs(5), runtime.wait_for_exit())
            .await
            .expect("crash reaped within bound")
            .expect("exit event published");
        assert_eq!(budget.live_processes(), 0);
    }

    struct CancelledLaunch {
        entered: tokio::sync::Notify,
    }

    #[async_trait]
    impl LspProcessLauncher for CancelledLaunch {
        async fn launch(
            &self,
            _spec: &LspLaunchSpec,
            _root: &Path,
        ) -> Result<LspProcess, LspError> {
            self.entered.notify_one();
            std::future::pending().await
        }
    }

    #[tokio::test]
    async fn cancelled_start_releases_admission_when_no_child_was_returned() {
        let budget = std::sync::Arc::new(LspLiveProcessBudget::new(1).expect("budget"));
        let inner = std::sync::Arc::new(CancelledLaunch {
            entered: tokio::sync::Notify::new(),
        });
        let launcher = std::sync::Arc::new(BudgetedLspProcessLauncher::new(
            inner.clone(),
            budget.clone(),
        ));
        let root = std::env::current_dir().expect("current test directory");
        let spec = LspLaunchSpec::default_for_test();
        let task = {
            let launcher = launcher.clone();
            tokio::spawn(async move { launcher.launch(&spec, &root).await })
        };
        inner.entered.notified().await;
        assert_eq!(budget.live_processes(), 1);
        task.abort();
        let _ = task.await;
        tokio::time::timeout(std::time::Duration::from_secs(2), async {
            while budget.live_processes() != 0 {
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("cancelled launch relinquishes its unused permit");
    }
}

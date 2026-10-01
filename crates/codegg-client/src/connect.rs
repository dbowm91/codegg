use crate::{ClientError, FrontendDescriptor, LocalSocketClient};
use codegg_protocol::core::{CoreRequest, CoreResponse, RequestEnvelope, PROTOCOL_VERSION};
use std::path::PathBuf;
use std::process::Stdio;
use std::time::Duration;
use tokio::process::Command;

#[derive(Debug, Clone)]
pub struct LocalDaemonOptions {
    pub endpoint: String,
    pub endpoint_argument: String,
    pub lock_path: PathBuf,
    pub log_path: PathBuf,
    pub executable: Option<PathBuf>,
    pub autostart: bool,
    pub startup_timeout: Duration,
    pub poll_interval: Duration,
}

pub struct LocalDaemonOutcome {
    pub client: LocalSocketClient,
    pub daemon_id: String,
    pub endpoint: String,
    pub started_pid: Option<u32>,
}

/// Connect to a verified daemon, or start the existing `codegg daemon start`
/// entry point and wait for a CoreFrame handshake plus daemon identity probe.
pub async fn connect_or_start_local_daemon(
    options: LocalDaemonOptions,
    descriptor: FrontendDescriptor,
) -> Result<LocalDaemonOutcome, ClientError> {
    std::fs::create_dir_all(
        options
            .log_path
            .parent()
            .unwrap_or_else(|| std::path::Path::new(".")),
    )
    .map_err(ClientError::Transport)?;
    let deadline = tokio::time::Instant::now() + options.startup_timeout;
    if let Some((client, daemon_id)) = verified_connect(&options, &descriptor, deadline).await {
        return Ok(LocalDaemonOutcome {
            client,
            daemon_id,
            endpoint: options.endpoint,
            started_pid: None,
        });
    }

    if !options.autostart {
        if lock_held(&options.lock_path) {
            return Err(ClientError::InconsistentSingleton(format!(
                "lock is held but endpoint {} is unreachable",
                options.endpoint
            )));
        }
        return Err(ClientError::Connect(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            format!(
                "no daemon running at {} (autostart disabled)",
                options.endpoint
            ),
        )));
    }

    let executable = options
        .executable
        .clone()
        .or_else(|| std::env::var_os("CODEGG_DAEMON_EXECUTABLE").map(PathBuf::from))
        .or_else(|| std::env::current_exe().ok())
        .ok_or_else(|| {
            ClientError::Transport(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "cannot resolve daemon executable",
            ))
        })?;
    let log = open_log(&options.log_path)?;
    let stderr = log.try_clone().map_err(ClientError::Transport)?;
    let mut command = Command::new(executable);
    command
        .args([
            "daemon",
            "start",
            "--endpoint",
            options.endpoint_argument.as_str(),
            "--force-take-lock",
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::from(log))
        .stderr(Stdio::from(stderr));
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.as_std_mut().process_group(0);
    }
    let mut child = command.spawn().map_err(ClientError::Transport)?;
    let pid = child.id();

    loop {
        if let Some((client, daemon_id)) = verified_connect(&options, &descriptor, deadline).await {
            tokio::spawn(async move {
                let _ = child.wait().await;
            });
            return Ok(LocalDaemonOutcome {
                client,
                daemon_id,
                endpoint: options.endpoint,
                started_pid: pid,
            });
        }
        if let Some(status) = child.try_wait().map_err(ClientError::Transport)? {
            // A competing launcher may have won after this child acquired its
            // process slot. Continue probing until the original deadline.
            while tokio::time::Instant::now() < deadline {
                if let Some((client, daemon_id)) =
                    verified_connect(&options, &descriptor, deadline).await
                {
                    return Ok(LocalDaemonOutcome {
                        client,
                        daemon_id,
                        endpoint: options.endpoint,
                        started_pid: pid,
                    });
                }
                tokio::time::sleep(
                    options
                        .poll_interval
                        .min(deadline.saturating_duration_since(tokio::time::Instant::now())),
                )
                .await;
            }
            return Err(ClientError::ChildExited(format!("exit status {status}")));
        }
        if tokio::time::Instant::now() >= deadline {
            let _ = child.kill().await;
            let _ = child.wait().await;
            return Err(ClientError::StartupTimeout);
        }
        tokio::time::sleep(options.poll_interval).await;
    }
}

async fn verified_connect(
    options: &LocalDaemonOptions,
    descriptor: &FrontendDescriptor,
    deadline: tokio::time::Instant,
) -> Option<(LocalSocketClient, String)> {
    // A stale or unrelated endpoint must not consume the entire startup
    // budget before we get a chance to launch the authoritative daemon.
    let probe_timeout = deadline
        .saturating_duration_since(tokio::time::Instant::now())
        .min(Duration::from_secs(1));
    if probe_timeout.is_zero() {
        return None;
    }
    tokio::time::timeout(probe_timeout, async {
        let client = LocalSocketClient::connect(&options.endpoint, descriptor.clone())
            .await
            .ok()?;
        let hello_daemon_id = client.daemon_id().await.ok()?;
        let request = RequestEnvelope {
            protocol_version: PROTOCOL_VERSION,
            request_id: uuid::Uuid::new_v4().to_string(),
            payload: CoreRequest::SnapshotDaemon,
        };
        match client.request(request).await.ok()? {
            CoreResponse::SnapshotDaemon { daemon_id, .. } if daemon_id == hello_daemon_id => {
                Some((client, daemon_id))
            }
            _ => None,
        }
    })
    .await
    .ok()
    .flatten()
}

fn lock_held(path: &std::path::Path) -> bool {
    use std::fs::OpenOptions;
    let file = match OpenOptions::new().read(true).write(true).open(path) {
        Ok(file) => file,
        Err(_) => return false,
    };
    match file.try_lock() {
        Ok(()) => {
            let _ = file.unlock();
            false
        }
        Err(std::fs::TryLockError::WouldBlock) => true,
        Err(std::fs::TryLockError::Error(_)) => false,
    }
}

fn open_log(path: &std::path::Path) -> Result<std::fs::File, ClientError> {
    const MAX_BYTES: u64 = 10 * 1024 * 1024;
    if std::fs::metadata(path).is_ok_and(|metadata| metadata.len() > MAX_BYTES) {
        let backup = path.with_extension("log.1");
        if let Err(error) = std::fs::rename(path, &backup) {
            tracing::warn!(%error, "failed to rotate daemon log");
        }
    }
    let mut options = std::fs::OpenOptions::new();
    options.create(true).append(true).read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    options.open(path).map_err(ClientError::Transport)
}

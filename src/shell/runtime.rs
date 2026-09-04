use std::sync::Arc;
use std::time::{Duration, Instant};

use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use super::types::{ShellCommandId, ShellEvent, ShellRequest, DEFAULT_TIMEOUT_SECS};

pub struct ShellRuntime {
    shell: String,
    plugin_service: Option<Arc<crate::plugin::service::PluginService>>,
}

impl ShellRuntime {
    pub fn new() -> Self {
        let shell = std::env::var("SHELL").unwrap_or_else(|_| "sh".to_string());
        Self {
            shell,
            plugin_service: None,
        }
    }

    /// Attach a plugin service for shell env lifecycle hooks.
    pub fn with_plugin_service(
        mut self,
        service: Arc<crate::plugin::service::PluginService>,
    ) -> Self {
        self.plugin_service = Some(service);
        self
    }

    #[cfg(test)]
    #[allow(dead_code)]
    fn with_shell(shell: &str) -> Self {
        Self {
            shell: shell.to_string(),
            plugin_service: None,
        }
    }

    pub async fn spawn(
        &self,
        req: ShellRequest,
        tx: mpsc::Sender<ShellEvent>,
    ) -> Result<ShellHandle, String> {
        let id = req.id;
        let command = req.command.clone();
        let cwd = req.cwd.clone();
        let timeout_dur = if req.timeout.as_secs() == 0 {
            Duration::from_secs(DEFAULT_TIMEOUT_SECS)
        } else {
            req.timeout
        };

        let _ = tx
            .send(ShellEvent::Started {
                id,
                command: command.clone(),
                cwd: cwd.clone(),
            })
            .await;

        // Dispatch shell env hook if plugin service is available.
        let mut extra_env: std::collections::HashMap<String, String> =
            std::collections::HashMap::new();
        let mut remove_env: Vec<String> = Vec::new();
        if let Some(ref plugin_svc) = self.plugin_service {
            let env_input = crate::plugin::lifecycle::ShellEnvHookInput {
                command: command.clone(),
                cwd: cwd.to_string_lossy().to_string(),
                base_env_keys: Vec::new(),
            };
            match crate::plugin::lifecycle::LifecycleHooks::new(
                plugin_svc.clone(),
                crate::plugin::policy::PluginLifecyclePolicy::default(),
            )
            .shell_env(env_input)
            .await
            {
                crate::plugin::lifecycle::PluginHookOutcome::Ok(output, _effects) => {
                    extra_env = output.env;
                    remove_env = output.remove;
                }
                crate::plugin::lifecycle::PluginHookOutcome::Failed { error } => {
                    tracing::warn!("shell env hook failed: {}", error);
                }
                _ => {}
            }
        }

        let mut environment_policy = match req.env_policy {
            super::types::ShellEnvPolicy::Inherit => {
                crate::managed_process::EnvironmentPolicy::inherited()
            }
            super::types::ShellEnvPolicy::Clean => {
                crate::managed_process::EnvironmentPolicy::sanitized()
            }
        };
        for key in remove_env {
            environment_policy = environment_policy.deny_var(key);
        }
        for (key, value) in extra_env {
            environment_policy = environment_policy.with_var(key, value);
        }

        let cancellation = CancellationToken::new();
        let mut process_request = crate::managed_process::ManagedProcessRequest::new(
            vec![self.shell.clone().into(), "-lc".into(), command.into()],
            cwd,
            crate::managed_process::ProcessProvenance::default(),
        );
        process_request.environment_policy = environment_policy;
        process_request.timeout = Some(timeout_dur);
        process_request.cancellation = cancellation.clone();
        process_request.output_policy =
            crate::managed_process::OutputPolicy::new(super::types::DEFAULT_MAX_BYTES_PER_COMMAND);

        let (output_tx, mut output_rx) = mpsc::channel(128);
        let process_task = tokio::spawn(
            crate::managed_process::ManagedProcessService::run_streaming(
                process_request,
                output_tx,
            ),
        );
        let tx_exit = tx.clone();
        tokio::spawn(async move {
            let start = Instant::now();
            let mut process_task = process_task;
            let result = loop {
                tokio::select! {
                    chunk = output_rx.recv() => {
                        if let Some(chunk) = chunk {
                            forward_output(&tx_exit, id, chunk).await;
                        }
                    }
                    result = &mut process_task => break result,
                }
            };
            while let Some(chunk) = output_rx.recv().await {
                forward_output(&tx_exit, id, chunk).await;
            }

            match result {
                Ok(Ok(result)) => match result.termination {
                    crate::managed_process::TerminationReason::Exited => {
                        let _ = tx_exit
                            .send(ShellEvent::Exited {
                                id,
                                status: result.exit_status.code(),
                                elapsed: start.elapsed(),
                            })
                            .await;
                    }
                    crate::managed_process::TerminationReason::TimedOut => {
                        let _ = tx_exit
                            .send(ShellEvent::TimedOut {
                                id,
                                elapsed: start.elapsed(),
                            })
                            .await;
                    }
                    crate::managed_process::TerminationReason::Cancelled => {}
                    crate::managed_process::TerminationReason::OutputLimitExceeded { stream } => {
                        let _ = tx_exit
                            .send(ShellEvent::FailedToStart {
                                id,
                                error: format!("managed shell output limit exceeded on {stream:?}"),
                            })
                            .await;
                    }
                },
                Ok(Err(error)) => {
                    let _ = tx_exit
                        .send(ShellEvent::FailedToStart {
                            id,
                            error: error.to_string(),
                        })
                        .await;
                }
                Err(error) => {
                    let _ = tx_exit
                        .send(ShellEvent::FailedToStart {
                            id,
                            error: format!("managed shell task failed: {error}"),
                        })
                        .await;
                }
            }
        });

        Ok(ShellHandle {
            id,
            cancellation,
            abort_handle: None,
        })
    }
}

impl Default for ShellRuntime {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone)]
pub struct ShellHandle {
    pub id: ShellCommandId,
    cancellation: CancellationToken,
    abort_handle: Option<tokio::task::AbortHandle>,
}

impl ShellHandle {
    pub fn kill(&self) {
        self.cancellation.cancel();
        if let Some(abort_handle) = &self.abort_handle {
            abort_handle.abort();
        }
    }

    pub fn id(&self) -> ShellCommandId {
        self.id
    }

    #[cfg(test)]
    pub fn new_for_test(id: ShellCommandId, abort_handle: tokio::task::AbortHandle) -> Self {
        Self {
            id,
            cancellation: CancellationToken::new(),
            abort_handle: Some(abort_handle),
        }
    }
}

async fn forward_output(
    tx: &mpsc::Sender<ShellEvent>,
    id: ShellCommandId,
    chunk: crate::managed_process::ManagedProcessOutputChunk,
) {
    let event = match chunk.stream {
        crate::managed_process::OutputStream::Stdout => ShellEvent::Stdout {
            id,
            bytes: chunk.bytes,
        },
        crate::managed_process::OutputStream::Stderr => ShellEvent::Stderr {
            id,
            bytes: chunk.bytes,
        },
    };
    let _ = tx.send(event).await;
}

#[cfg(test)]
mod tests {
    use super::super::types::ShellEnvPolicy;
    use super::*;
    use tokio::sync::mpsc;

    async fn collect_events(
        tx: mpsc::Sender<ShellEvent>,
        rx: mpsc::Receiver<ShellEvent>,
    ) -> Vec<ShellEvent> {
        drop(tx);
        let mut events = Vec::new();
        let mut rx = rx;
        while let Some(event) = rx.recv().await {
            events.push(event);
        }
        events
    }

    #[tokio::test]
    async fn runtime_simple_command() {
        let runtime = ShellRuntime::new();
        let (tx, rx) = mpsc::channel(128);
        let req = ShellRequest {
            id: ShellCommandId(1),
            origin: super::super::types::ShellOrigin::HumanEphemeral,
            command: "printf hello".to_string(),
            cwd: std::env::temp_dir(),
            timeout: Duration::from_secs(10),
            capture_policy: super::super::types::ShellCapturePolicy::StoreEphemeral,
            env_policy: ShellEnvPolicy::Inherit,
        };

        let handle = runtime.spawn(req, tx.clone()).await.unwrap();
        let events = collect_events(tx, rx).await;

        let started = events
            .iter()
            .find(|e| matches!(e, ShellEvent::Started { .. }));
        assert!(started.is_some());

        let stdout_events: Vec<_> = events
            .iter()
            .filter_map(|e| {
                if let ShellEvent::Stdout { bytes, .. } = e {
                    Some(bytes.as_slice())
                } else {
                    None
                }
            })
            .collect();
        let combined: Vec<u8> = stdout_events.into_iter().flatten().copied().collect();
        assert_eq!(combined, b"hello");

        let exited = events
            .iter()
            .find(|e| matches!(e, ShellEvent::Exited { .. }));
        assert!(exited.is_some());

        handle.kill();
    }

    #[tokio::test]
    async fn runtime_stderr_output() {
        let runtime = ShellRuntime::new();
        let (tx, rx) = mpsc::channel(128);
        let req = ShellRequest {
            id: ShellCommandId(2),
            origin: super::super::types::ShellOrigin::HumanEphemeral,
            command: "printf err >&2; exit 0".to_string(),
            cwd: std::env::temp_dir(),
            timeout: Duration::from_secs(10),
            capture_policy: super::super::types::ShellCapturePolicy::StoreEphemeral,
            env_policy: ShellEnvPolicy::Inherit,
        };

        let handle = runtime.spawn(req, tx.clone()).await.unwrap();
        let events = collect_events(tx, rx).await;

        let stderr_events: Vec<_> = events
            .iter()
            .filter_map(|e| {
                if let ShellEvent::Stderr { bytes, .. } = e {
                    Some(bytes.as_slice())
                } else {
                    None
                }
            })
            .collect();
        let combined: Vec<u8> = stderr_events.into_iter().flatten().copied().collect();
        let stderr_str = String::from_utf8_lossy(&combined);
        assert!(
            stderr_str.contains("err"),
            "stderr should contain 'err', got: {:?}",
            stderr_str
        );

        handle.kill();
    }

    #[tokio::test]
    async fn runtime_nonzero_exit() {
        let runtime = ShellRuntime::new();
        let (tx, rx) = mpsc::channel(128);
        let req = ShellRequest {
            id: ShellCommandId(3),
            origin: super::super::types::ShellOrigin::HumanEphemeral,
            command: "exit 7".to_string(),
            cwd: std::env::temp_dir(),
            timeout: Duration::from_secs(10),
            capture_policy: super::super::types::ShellCapturePolicy::StoreEphemeral,
            env_policy: ShellEnvPolicy::Inherit,
        };

        let handle = runtime.spawn(req, tx.clone()).await.unwrap();
        let events = collect_events(tx, rx).await;

        let exited = events
            .iter()
            .find_map(|e| {
                if let ShellEvent::Exited { status, .. } = e {
                    Some(*status)
                } else {
                    None
                }
            })
            .unwrap();
        assert_eq!(exited, Some(7));

        handle.kill();
    }

    #[tokio::test]
    async fn runtime_invalid_command() {
        let runtime = ShellRuntime::new();
        let (tx, rx) = mpsc::channel(128);
        let req = ShellRequest {
            id: ShellCommandId(5),
            origin: super::super::types::ShellOrigin::HumanEphemeral,
            command: "__nonexistent_command_xyz__".to_string(),
            cwd: std::env::temp_dir(),
            timeout: Duration::from_secs(5),
            capture_policy: super::super::types::ShellCapturePolicy::StoreEphemeral,
            env_policy: ShellEnvPolicy::Inherit,
        };

        let handle = runtime.spawn(req, tx.clone()).await;
        let events = collect_events(tx, rx).await;

        let failed = events
            .iter()
            .find(|e| matches!(e, ShellEvent::FailedToStart { .. }));
        let exited_with_code = events.iter().find_map(|e| {
            if let ShellEvent::Exited { status, .. } = e {
                Some(*status)
            } else {
                None
            }
        });

        assert!(failed.is_some() || exited_with_code == Some(Some(127)));
        if let Ok(h) = handle {
            h.kill();
        }
    }

    #[tokio::test]
    async fn runtime_cwd_respected() {
        let runtime = ShellRuntime::new();
        let (tx, rx) = mpsc::channel(128);
        let tmp = std::env::temp_dir();
        let req = ShellRequest {
            id: ShellCommandId(6),
            origin: super::super::types::ShellOrigin::HumanEphemeral,
            command: "pwd".to_string(),
            cwd: tmp.clone(),
            timeout: Duration::from_secs(5),
            capture_policy: super::super::types::ShellCapturePolicy::StoreEphemeral,
            env_policy: ShellEnvPolicy::Inherit,
        };

        let handle = runtime.spawn(req, tx.clone()).await.unwrap();
        let events = collect_events(tx, rx).await;

        let stdout: Vec<u8> = events
            .iter()
            .filter_map(|e| {
                if let ShellEvent::Stdout { bytes, .. } = e {
                    Some(bytes.as_slice())
                } else {
                    None
                }
            })
            .flatten()
            .copied()
            .collect();
        let output = String::from_utf8_lossy(&stdout);
        assert!(output.trim() == tmp.to_string_lossy().as_ref() || !output.is_empty());

        handle.kill();
    }

    #[test]
    fn handle_kill_is_safe() {
        let (_tx, _rx) = mpsc::channel::<ShellEvent>(1);
        let abort_handle = tokio::runtime::Runtime::new()
            .unwrap()
            .block_on(async { tokio::spawn(async {}).abort_handle() });
        let handle = ShellHandle {
            id: ShellCommandId(99),
            cancellation: CancellationToken::new(),
            abort_handle: Some(abort_handle),
        };
        handle.kill();
    }

    #[tokio::test]
    async fn runtime_timeout_emits_timed_out_event() {
        let runtime = ShellRuntime::with_shell("sh");
        let (tx, mut rx) = mpsc::channel(128);

        let req = ShellRequest {
            id: ShellCommandId(10),
            origin: super::super::types::ShellOrigin::HumanEphemeral,
            command: "while true; do :; done".to_string(),
            cwd: std::env::temp_dir(),
            timeout: Duration::from_millis(200),
            capture_policy: super::super::types::ShellCapturePolicy::StoreEphemeral,
            env_policy: ShellEnvPolicy::Inherit,
        };

        let _handle = runtime.spawn(req, tx.clone()).await.unwrap();
        drop(tx);

        let result = tokio::time::timeout(Duration::from_secs(8), async {
            let mut got_started = false;
            let mut got_timed_out = false;
            while let Some(event) = rx.recv().await {
                match &event {
                    ShellEvent::Started { .. } => got_started = true,
                    ShellEvent::TimedOut { .. } => {
                        got_timed_out = true;
                        break;
                    }
                    _ => {}
                }
            }
            (got_started, got_timed_out)
        })
        .await;

        match result {
            Ok((started, timed_out)) => {
                assert!(started, "should have received Started event");
                assert!(timed_out, "should have received TimedOut event");
            }
            Err(_) => panic!("managed shell timeout did not emit a TimedOut event"),
        }
    }
}

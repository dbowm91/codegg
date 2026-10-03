//! Deterministic control seam for the C003 built-app WebDriver trajectory.
//!
//! This helper is a separate test process that talks to an isolated daemon
//! through the same native protocol/client crates as production (`codegg-client`
//! + `codegg-protocol`). It holds a TUI-kind observer client against the
//! isolated daemon, so the WebDriver spec can assert real daemon
//! `connected_clients` counts, register deterministic projects, and emit
//! project-catalog invalidations on demand.
//!
//! Transport is a Unix-domain socket (not stdio): the `@wdio/tauri-service`
//! spawns the desktop app once per WebdriverIO invocation in the launcher
//! process, so spec code in workers cannot own the fixture's stdio. Instead
//! `e2e/run-e2e.sh` starts `desktop_e2e_fixture serve` per phase, and every
//! party (the phase script, the specs) sends newline-delimited JSON commands
//! over the socket. Each connection carries `{"id":…,"cmd":…}` objects;
//! responses are `{"id":…,"ok":true,…}` or `{"id":…,"ok":false,"error":"…"}`.
//! Commands: `start`, `snapshot`, `register_project`, `archive_project`,
//! `restore_project`, `reattach`, `stop_daemon`, `shutdown`.
//!
//! It never touches the operator's real daemon home or projects:
//! - `CODEGG_E2E_HOME` (required) must be temp-scoped;
//! - `CODEGG_DAEMON_EXECUTABLE` (required) must be an existing daemon binary
//!   built from the tested revision;
//! - the helper creates `{home}/daemon-home` + `{home}/workspace` itself and
//!   removes `{home}` on `shutdown` (best effort on crash too);
//! - daemon kills are identity-checked (live daemon id must equal the id this
//!   helper autostarted, back-to-back with the signal) so a recycled pid can
//!   never receive the signal. When no autostart pid is known (the desktop
//!   app autostarted the daemon), shutdown falls back to a best-effort
//!   `pkill -f` scoped to the phase's unique socket path, then removes the
//!   home regardless.
//!
//! Unix-only (the trajectory is Linux/macOS; Windows remains M002-gated).
//! Built without any Tauri dependency; the WebDriver spec in
//! `apps/desktop/e2e/` drives it. It is not part of the production desktop
//! bundle (a `src/bin/` helper used only by the E2E harness).

#[cfg(unix)]
mod server {
    use codegg_client::{
        connect_or_start_local_daemon, FrontendDescriptor, LocalDaemonOptions, LocalDaemonOutcome,
        LocalDaemonPaths, LocalSocketClient,
    };
    use codegg_protocol::{
        core::{CoreRequest, CoreResponse, RequestEnvelope, PROTOCOL_VERSION},
        dto::ProjectRegisterRequestDto,
        frames::{ClientCapabilities, ClientKind},
    };
    use serde_json::{json, Value};
    use std::{path::PathBuf, sync::Arc, time::Duration};
    use tokio::{
        io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
        net::{UnixListener, UnixStream},
        sync::{Mutex, Notify},
    };

    const STARTUP_TIMEOUT: Duration = Duration::from_secs(20);
    const REQUEST_TIMEOUT: Duration = Duration::from_secs(10);
    const POLL_INTERVAL: Duration = Duration::from_millis(200);
    const DAEMON_DEATH_TIMEOUT: Duration = Duration::from_secs(10);

    fn fail(id: &Value, message: impl Into<String>) -> Value {
        json!({"id": id, "ok": false, "error": message.into()})
    }

    fn tmp_display() -> String {
        std::env::temp_dir().display().to_string()
    }

    fn is_temp_scoped(home: &std::path::Path) -> bool {
        let tmp = std::env::temp_dir();
        if home == tmp || home.starts_with(&tmp) {
            return true;
        }
        // macOS: TMPDIR resolves under /var/folders/… while callers often stage
        // under /tmp (/private/tmp). Both are OS temp scope.
        for dir in ["/tmp", "/private/tmp"] {
            let dir = PathBuf::from(dir);
            if home == dir || home.starts_with(&dir) {
                return true;
            }
        }
        false
    }

    fn observer_descriptor() -> FrontendDescriptor {
        FrontendDescriptor::new(
            "codegg-e2e-fixture",
            ClientKind::Tui,
            ClientCapabilities {
                visual_notifications: false,
                desktop_notifications: false,
                audio: false,
                tts: false,
                multi_session_view: false,
                plugin_ui_dialog: false,
                plugin_ui_toast: false,
                plugin_ui_panel: false,
                plugin_ui_status_item: false,
                plugin_ui_table: false,
                plugin_ui_markdown: false,
                plugin_ui_code: false,
                plugin_ui_progress: false,
                workspace_registration: true,
                project_catalog: true,
                session_projection: false,
            },
        )
    }

    struct Fixture {
        home: PathBuf,
        socket_path: PathBuf,
        workspace_root: PathBuf,
        daemon_executable: PathBuf,
        observer: Option<LocalDaemonOutcome>,
        workspace_id: Option<String>,
        shutdown: Arc<Notify>,
    }

    impl Fixture {
        fn new(shutdown: Arc<Notify>) -> Result<Self, String> {
            let home = std::env::var_os("CODEGG_E2E_HOME")
                .map(PathBuf::from)
                .ok_or_else(|| "CODEGG_E2E_HOME is required".to_owned())?;
            if !is_temp_scoped(&home) {
                return Err(format!(
                    "CODEGG_E2E_HOME must be temp-scoped ({} or /tmp), got {}",
                    tmp_display(),
                    home.display()
                ));
            }
            let daemon_executable = std::env::var_os("CODEGG_DAEMON_EXECUTABLE")
                .map(PathBuf::from)
                .ok_or_else(|| "CODEGG_DAEMON_EXECUTABLE is required".to_owned())?;
            if !daemon_executable.is_file() {
                return Err(format!(
                    "daemon executable is missing: {}",
                    daemon_executable.display()
                ));
            }
            let daemon_home = home.join("daemon-home");
            let workspace_root = home.join("workspace");
            std::fs::create_dir_all(&daemon_home)
                .map_err(|e| format!("create daemon home: {e}"))?;
            std::fs::create_dir_all(&workspace_root)
                .map_err(|e| format!("create workspace root: {e}"))?;
            // Process-local daemon-home isolation for LocalDaemonPaths::resolve().
            std::env::set_var("CODEGG_DAEMON_HOME", &daemon_home);
            Ok(Self {
                socket_path: home.join("fixture.sock"),
                home,
                workspace_root,
                daemon_executable,
                observer: None,
                workspace_id: None,
                shutdown,
            })
        }

        fn observer_client(&self) -> Result<&LocalSocketClient, String> {
            self.observer
                .as_ref()
                .map(|outcome| &outcome.client)
                .ok_or_else(|| "fixture is not started".to_owned())
        }

        async fn request(&self, payload: CoreRequest) -> Result<CoreResponse, String> {
            let client = self.observer_client()?;
            tokio::time::timeout(
                REQUEST_TIMEOUT,
                client.request(RequestEnvelope {
                    protocol_version: PROTOCOL_VERSION,
                    request_id: uuid::Uuid::new_v4().to_string(),
                    payload,
                }),
            )
            .await
            .map_err(|_| "daemon request timed out".to_owned())?
            .map_err(|e| format!("daemon request failed: {e}"))
        }

        async fn snapshot_value(&self) -> Result<Value, String> {
            let response = self.request(CoreRequest::SnapshotDaemon).await?;
            match response {
                CoreResponse::SnapshotDaemon {
                    daemon_id,
                    connected_clients,
                    ..
                } => {
                    let desktop = connected_clients
                        .iter()
                        .filter(|client| client.client_name == "codegg-desktop")
                        .count();
                    Ok(json!({
                        "daemon_id": daemon_id,
                        "total_clients": connected_clients.len(),
                        "desktop_clients": desktop,
                        "clients": connected_clients.iter().map(|client| json!({
                            "client_id": client.client_id,
                            "client_name": client.client_name,
                        })).collect::<Vec<_>>(),
                    }))
                }
                other => Err(format!("unexpected snapshot response: {other:?}")),
            }
        }

        async fn connect_observer(&mut self) -> Result<(), String> {
            let paths = LocalDaemonPaths::resolve();
            if let Err(e) = paths.ensure_root() {
                return Err(format!("ensure daemon home: {e}"));
            }
            let endpoint = paths.endpoint_uri();
            let outcome = connect_or_start_local_daemon(
                LocalDaemonOptions {
                    endpoint,
                    endpoint_argument: paths.socket_path_str(),
                    lock_path: paths.lock_path.clone(),
                    log_path: paths.log_path.clone(),
                    executable: Some(self.daemon_executable.clone()),
                    autostart: true,
                    startup_timeout: STARTUP_TIMEOUT,
                    poll_interval: POLL_INTERVAL,
                },
                observer_descriptor(),
            )
            .await
            .map_err(|e| format!("observer autostart failed: {e}"))?;
            self.observer = Some(outcome);
            Ok(())
        }

        async fn ensure_workspace(&mut self) -> Result<String, String> {
            if let Some(workspace_id) = self.workspace_id.clone() {
                return Ok(workspace_id);
            }
            let client = self.observer_client()?;
            let response = tokio::time::timeout(
                REQUEST_TIMEOUT,
                client.request(RequestEnvelope {
                    protocol_version: PROTOCOL_VERSION,
                    request_id: uuid::Uuid::new_v4().to_string(),
                    payload: CoreRequest::WorkspaceRegister {
                        root: self.workspace_root.to_string_lossy().into_owned(),
                    },
                }),
            )
            .await
            .map_err(|_| "workspace register timed out".to_owned())?
            .map_err(|e| format!("workspace register failed: {e}"))?;
            match response {
                CoreResponse::WorkspaceSnapshot { workspace } => {
                    self.workspace_id = Some(workspace.workspace_id.clone());
                    Ok(workspace.workspace_id)
                }
                other => Err(format!("unexpected workspace response: {other:?}")),
            }
        }

        async fn register_project_inner(&self, display_name: &str) -> Result<String, String> {
            let workspace_id = self
                .workspace_id
                .clone()
                .ok_or_else(|| "workspace is not registered".to_owned())?;
            let response = self
                .request(CoreRequest::ProjectRegister {
                    request: ProjectRegisterRequestDto {
                        workspace_id,
                        display_name: display_name.to_owned(),
                        description: None,
                        tags: Vec::new(),
                        repository_id: None,
                        source: "desktop-e2e-fixture".into(),
                    },
                })
                .await?;
            match response {
                CoreResponse::ProjectRegistered { project } => Ok(project.project_id),
                other => Err(format!("unexpected register response: {other:?}")),
            }
        }

        async fn cmd_start(&mut self, id: &Value) -> Value {
            // Idempotent: reuse the held observer when it is still alive so
            // the phase script and the specs can share one daemon/session.
            let mut fresh = false;
            if self.observer.is_none() {
                if let Err(e) = self.connect_observer().await {
                    return fail(id, e);
                }
                fresh = true;
            } else if let Err(error) = self.snapshot_value().await {
                // Stale observer (daemon restarted underneath us): reattach.
                eprintln!("fixture observer stale ({error}); reattaching");
                self.observer = None;
                if let Err(e) = self.connect_observer().await {
                    return fail(id, e);
                }
                fresh = true;
            }
            let observer = self.observer.as_ref().expect("observer connected");
            let daemon_id = observer.daemon_id.clone();
            let started_pid = observer.started_pid;
            let endpoint = LocalDaemonPaths::resolve().endpoint_uri();
            if let Err(e) = self.ensure_workspace().await {
                return fail(id, e);
            }
            let workspace_id = self.workspace_id.clone().expect("workspace registered");
            if fresh {
                let project_name = format!("e2e-probe-{}", std::process::id());
                match self.register_project_inner(&project_name).await {
                    Ok(project_id) => json!({
                        "id": id,
                        "ok": true,
                        "daemon_id": daemon_id,
                        "endpoint": endpoint,
                        "started_pid": started_pid,
                        "workspace_id": workspace_id,
                        "project_id": project_id,
                        "project_name": project_name,
                    }),
                    Err(e) => fail(id, e),
                }
            } else {
                json!({
                    "id": id,
                    "ok": true,
                    "daemon_id": daemon_id,
                    "endpoint": endpoint,
                    "started_pid": started_pid,
                    "workspace_id": workspace_id,
                })
            }
        }

        async fn cmd_snapshot(&self, id: &Value) -> Value {
            match self.snapshot_value().await {
                Ok(snapshot) => json!({"id": id, "ok": true, "snapshot": snapshot}),
                Err(e) => fail(id, e),
            }
        }

        async fn cmd_register_project(&self, id: &Value, cmd: &Value) -> Value {
            let display_name = cmd
                .get("display_name")
                .and_then(Value::as_str)
                .unwrap_or("e2e-extra-probe");
            match self.register_project_inner(display_name).await {
                Ok(project_id) => json!({"id": id, "ok": true, "project_id": project_id}),
                Err(e) => fail(id, e),
            }
        }

        async fn cmd_archive_project(&self, id: &Value, cmd: &Value) -> Value {
            let Some(project_id) = cmd.get("project_id").and_then(Value::as_str) else {
                return fail(id, "archive_project requires project_id");
            };
            match self
                .request(CoreRequest::ProjectArchive {
                    project_id: project_id.to_owned(),
                })
                .await
            {
                Ok(CoreResponse::ProjectArchived { .. }) => json!({"id": id, "ok": true}),
                Ok(other) => fail(id, format!("unexpected archive response: {other:?}")),
                Err(e) => fail(id, e),
            }
        }

        async fn cmd_restore_project(&self, id: &Value, cmd: &Value) -> Value {
            let Some(project_id) = cmd.get("project_id").and_then(Value::as_str) else {
                return fail(id, "restore_project requires project_id");
            };
            match self
                .request(CoreRequest::ProjectRestore {
                    project_id: project_id.to_owned(),
                })
                .await
            {
                Ok(CoreResponse::ProjectRestored { .. }) => json!({"id": id, "ok": true}),
                Ok(other) => fail(id, format!("unexpected restore response: {other:?}")),
                Err(e) => fail(id, e),
            }
        }

        /// Drop the (possibly dead) observer and attach a fresh TUI-kind
        /// observer to the daemon in the same isolated home — used after the
        /// desktop app autostarts a new daemon there.
        async fn cmd_reattach(&mut self, id: &Value) -> Value {
            self.observer = None;
            self.workspace_id = None;
            match self.connect_observer().await {
                Ok(()) => {
                    let daemon_id = self
                        .observer
                        .as_ref()
                        .expect("observer connected")
                        .daemon_id
                        .clone();
                    json!({"id": id, "ok": true, "daemon_id": daemon_id})
                }
                Err(e) => fail(id, e),
            }
        }

        /// Identity-checked daemon stop: the live daemon id must equal the id
        /// this fixture autostarted, back-to-back with the kill, so a recycled
        /// pid can never receive the signal.
        async fn stop_daemon_inner(&self) -> Result<(), String> {
            let observer = self
                .observer
                .as_ref()
                .ok_or_else(|| "fixture is not started".to_owned())?;
            let live = tokio::time::timeout(
                REQUEST_TIMEOUT,
                observer.client.request(RequestEnvelope {
                    protocol_version: PROTOCOL_VERSION,
                    request_id: uuid::Uuid::new_v4().to_string(),
                    payload: CoreRequest::SnapshotDaemon,
                }),
            )
            .await
            .map_err(|_| "pre-kill snapshot timed out".to_owned())?
            .map_err(|e| format!("pre-kill snapshot failed: {e}"))?;
            match &live {
                CoreResponse::SnapshotDaemon { daemon_id, .. }
                    if *daemon_id == observer.daemon_id => {}
                _ => {
                    return Err(format!(
                        "daemon identity changed before kill (expected {})",
                        observer.daemon_id
                    ));
                }
            }
            let pid = observer
                .started_pid
                .ok_or_else(|| "no autostarted pid to stop".to_owned())?;
            let status = std::process::Command::new("kill")
                .arg(pid.to_string())
                .status()
                .map_err(|e| format!("kill failed: {e}"))?;
            if !status.success() {
                return Err(format!("kill exit: {status:?}"));
            }
            let deadline = tokio::time::Instant::now() + DAEMON_DEATH_TIMEOUT;
            loop {
                let dead = match tokio::time::timeout(
                    REQUEST_TIMEOUT,
                    observer.client.request(RequestEnvelope {
                        protocol_version: PROTOCOL_VERSION,
                        request_id: uuid::Uuid::new_v4().to_string(),
                        payload: CoreRequest::SnapshotDaemon,
                    }),
                )
                .await
                {
                    Err(_) => true,
                    Ok(Err(_)) => true,
                    Ok(Ok(_)) => false,
                };
                if dead {
                    return Ok(());
                }
                if tokio::time::Instant::now() >= deadline {
                    return Err("daemon did not stop".to_owned());
                }
                tokio::time::sleep(POLL_INTERVAL).await;
            }
        }

        async fn cmd_stop_daemon(&mut self, id: &Value) -> Value {
            let result = self.stop_daemon_inner().await;
            // The observer connection died with the daemon either way.
            self.observer = None;
            match result {
                Ok(()) => json!({"id": id, "ok": true}),
                Err(e) => fail(id, e),
            }
        }

        /// Best-effort scoped kill for daemons the fixture did not autostart
        /// (the desktop app autostarted them): match the phase's unique daemon
        /// socket path in the full command line. The bracket trick keeps the
        /// pattern from matching our own invocation. Non-fatal by design.
        fn scoped_pkill(&self) -> bool {
            let socket = LocalDaemonPaths::resolve().socket_path_str();
            let mut pattern = socket.clone();
            let Some(pos) = pattern.rfind(".sock") else {
                return false;
            };
            pattern.replace_range(pos..pos + 1, "[.]");
            std::process::Command::new("pkill")
                .arg("-f")
                .arg(&pattern)
                .status()
                .map(|status| status.success())
                .unwrap_or(false)
        }

        fn cleanup(&mut self) {
            if let Some(observer) = &self.observer {
                if let Some(pid) = observer.started_pid {
                    let _ = std::process::Command::new("kill")
                        .arg(pid.to_string())
                        .status();
                }
            } else {
                // No autostart pid is known (the desktop app may have
                // autostarted a daemon here): scoped best-effort kill.
                let _ = self.scoped_pkill();
            }
            self.observer = None;
            let _ = std::fs::remove_dir_all(&self.home);
        }

        async fn handle(&mut self, cmd: &Value) -> Option<Value> {
            let id = cmd.get("id").cloned().unwrap_or(Value::Null);
            let name = cmd.get("cmd").and_then(Value::as_str).unwrap_or("");
            match name {
                "start" => Some(self.cmd_start(&id).await),
                "snapshot" => Some(self.cmd_snapshot(&id).await),
                "register_project" => Some(self.cmd_register_project(&id, cmd).await),
                "archive_project" => Some(self.cmd_archive_project(&id, cmd).await),
                "restore_project" => Some(self.cmd_restore_project(&id, cmd).await),
                "reattach" => Some(self.cmd_reattach(&id).await),
                "stop_daemon" => Some(self.cmd_stop_daemon(&id).await),
                "shutdown" => {
                    self.cleanup();
                    let response = json!({"id": id, "ok": true});
                    self.shutdown.notify_one();
                    Some(response)
                }
                _ => Some(fail(&id, format!("unknown command: {name}"))),
            }
        }
    }

    async fn handle_connection(fixture: Arc<Mutex<Fixture>>, stream: UnixStream) {
        let (reader, mut writer) = stream.into_split();
        let mut lines = BufReader::new(reader).lines();
        loop {
            let line = match lines.next_line().await {
                Ok(Some(line)) => line,
                _ => break,
            };
            if line.trim().is_empty() {
                continue;
            }
            let cmd: Value = match serde_json::from_str(&line) {
                Ok(cmd) => cmd,
                Err(e) => {
                    let response = json!({"id": Value::Null, "ok": false, "error": format!("invalid command JSON: {e}")});
                    if writer
                        .write_all(response.to_string().as_bytes())
                        .await
                        .is_err()
                    {
                        break;
                    }
                    if writer.write_all(b"\n").await.is_err() {
                        break;
                    }
                    continue;
                }
            };
            let response = fixture.lock().await.handle(&cmd).await;
            if let Some(response) = response {
                if writer
                    .write_all(response.to_string().as_bytes())
                    .await
                    .is_err()
                {
                    break;
                }
                if writer.write_all(b"\n").await.is_err() {
                    break;
                }
                if writer.flush().await.is_err() {
                    break;
                }
            }
        }
    }

    pub async fn serve() -> Result<(), String> {
        let shutdown = Arc::new(Notify::new());
        let fixture = Fixture::new(Arc::clone(&shutdown))?;
        let socket_path = fixture.socket_path.clone();
        let _ = std::fs::remove_file(&socket_path);
        let listener =
            UnixListener::bind(&socket_path).map_err(|e| format!("bind fixture socket: {e}"))?;
        println!(
            "{}",
            json!({"ok": true, "ready": true, "socket": socket_path.display().to_string()})
        );
        use std::io::Write as _;
        let _ = std::io::stdout().flush();
        let fixture = Arc::new(Mutex::new(fixture));
        loop {
            tokio::select! {
                _ = shutdown.notified() => break,
                accepted = listener.accept() => {
                    match accepted {
                        Ok((stream, _)) => {
                            let fixture = Arc::clone(&fixture);
                            tokio::spawn(handle_connection(fixture, stream));
                        }
                        Err(e) => {
                            eprintln!("fixture accept failed: {e}");
                            break;
                        }
                    }
                }
            }
        }
        Ok(())
    }
}

#[cfg(unix)]
#[tokio::main(flavor = "current_thread")]
async fn main() {
    let serve = std::env::args().nth(1).as_deref() == Some("serve");
    if !serve {
        eprintln!("usage: desktop_e2e_fixture serve");
        std::process::exit(2);
    }
    if let Err(e) = server::serve().await {
        println!("{}", serde_json::json!({"ok": false, "error": e}));
        std::process::exit(2);
    }
}

#[cfg(not(unix))]
fn main() {
    eprintln!("desktop_e2e_fixture is Unix-only (the built-app trajectory targets Linux/macOS)");
    std::process::exit(2);
}

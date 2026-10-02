//! Deterministic control seam for the C003 built-app WebDriver trajectory.
//!
//! This helper is a separate test process that talks to an isolated daemon
//! through the same native protocol/client crates as production (`codegg-client`
//! + `codegg-protocol`). It owns a TUI-kind observer client for the whole
//! trajectory so the WebDriver spec can assert real daemon `connected_clients`
//! counts, register deterministic projects, and emit project-catalog
//! invalidations on demand.
//!
//! It never touches the operator's real daemon home or projects:
//! - `CODEGG_E2E_HOME` (required) must resolve under the OS temp directory;
//! - `CODEGG_DAEMON_EXECUTABLE` (required) must be an existing daemon binary
//!   built from the tested revision;
//! - the helper creates `{home}/daemon-home` + `{home}/workspace` itself and
//!   removes `{home}` on `shutdown` (best effort on EOF/crash too);
//! - daemon kills are identity-checked (live daemon id must equal the id this
//!   helper autostarted, back-to-back with the signal) so a recycled pid can
//!   never receive the signal.
//!
//! Stdio protocol is one JSON object per line on stdin, one JSON response per
//! line on stdout. Every command carries an `id`; responses are
//! `{"id":…,"ok":true,…}` or `{"id":…,"ok":false,"error":"…"}`.
//! Commands: `start`, `snapshot`, `register_project`, `archive_project`,
//! `restore_project`, `stop_daemon`, `reattach`, `shutdown`.
//!
//! Built without any Tauri dependency; the WebDriver spec in
//! `apps/desktop/e2e/` drives it. It is not part of the production desktop
//! bundle (a `src/bin/` helper used only by the E2E harness).

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
use std::{
    io::{BufRead, Write},
    path::PathBuf,
    time::Duration,
};

const STARTUP_TIMEOUT: Duration = Duration::from_secs(20);
const REQUEST_TIMEOUT: Duration = Duration::from_secs(10);
const POLL_INTERVAL: Duration = Duration::from_millis(200);
const DAEMON_DEATH_TIMEOUT: Duration = Duration::from_secs(10);

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

fn fail(id: &Value, message: impl Into<String>) -> Value {
    json!({"id": id, "ok": false, "error": message.into()})
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
    workspace_root: PathBuf,
    daemon_executable: PathBuf,
    observer: Option<LocalDaemonOutcome>,
    workspace_id: Option<String>,
}

impl Fixture {
    fn new() -> Result<Self, String> {
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
        std::fs::create_dir_all(&daemon_home).map_err(|e| format!("create daemon home: {e}"))?;
        std::fs::create_dir_all(&workspace_root)
            .map_err(|e| format!("create workspace root: {e}"))?;
        // Process-local daemon-home isolation for LocalDaemonPaths::resolve().
        std::env::set_var("CODEGG_DAEMON_HOME", &daemon_home);
        Ok(Self {
            home,
            workspace_root,
            daemon_executable,
            observer: None,
            workspace_id: None,
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

    async fn cmd_start(&mut self, id: &Value) -> Value {
        if self.observer.is_some() {
            return fail(id, "fixture is already started");
        }
        let paths = LocalDaemonPaths::resolve();
        if let Err(e) = paths.ensure_root() {
            return fail(id, format!("ensure daemon home: {e}"));
        }
        let endpoint = paths.endpoint_uri();
        let outcome = match connect_or_start_local_daemon(
            LocalDaemonOptions {
                endpoint: endpoint.clone(),
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
        {
            Ok(outcome) => outcome,
            Err(e) => return fail(id, format!("observer autostart failed: {e}")),
        };
        // Deterministic temporary workspace + probe project in LocalOwner test
        // context, registered through the observer peer.
        let workspace_id = match self.register_workspace_via(&outcome.client).await {
            Ok(workspace_id) => workspace_id,
            Err(e) => return fail(id, e),
        };
        let daemon_id = outcome.daemon_id.clone();
        let started_pid = outcome.started_pid;
        self.workspace_id = Some(workspace_id.clone());
        self.observer = Some(outcome);
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
    }

    async fn register_workspace_via(&self, client: &LocalSocketClient) -> Result<String, String> {
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
            CoreResponse::WorkspaceSnapshot { workspace } => Ok(workspace.workspace_id),
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
            CoreResponse::SnapshotDaemon { daemon_id, .. } if *daemon_id == observer.daemon_id => {}
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

    async fn cmd_stop_daemon(&self, id: &Value) -> Value {
        match self.stop_daemon_inner().await {
            Ok(()) => json!({"id": id, "ok": true}),
            Err(e) => fail(id, e),
        }
    }

    /// Drop the (possibly dead) observer connection and attach a fresh
    /// TUI-kind observer to the daemon in the same isolated home — used
    /// after the desktop app autostarts a new daemon there.
    async fn cmd_reattach(&mut self, id: &Value) -> Value {
        self.observer = None;
        let paths = LocalDaemonPaths::resolve();
        let endpoint = paths.endpoint_uri();
        match connect_or_start_local_daemon(
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
        {
            Ok(outcome) => {
                let daemon_id = outcome.daemon_id.clone();
                self.observer = Some(outcome);
                json!({"id": id, "ok": true, "daemon_id": daemon_id})
            }
            Err(e) => fail(id, format!("observer reattach failed: {e}")),
        }
    }

    fn cleanup(&mut self) {
        // Best-effort identity-checked stop is only possible while the
        // observer connection is alive; the isolated home is always removed.
        if let Some(observer) = &self.observer {
            if let Some(pid) = observer.started_pid {
                let _ = std::process::Command::new("kill")
                    .arg(pid.to_string())
                    .status();
            }
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
            "stop_daemon" => Some(self.cmd_stop_daemon(&id).await),
            "reattach" => Some(self.cmd_reattach(&id).await),
            "shutdown" => {
                self.cleanup();
                Some(json!({"id": id, "ok": true}))
            }
            _ => Some(fail(&id, format!("unknown command: {name}"))),
        }
    }
}

#[tokio::main(flavor = "current_thread")]
async fn main() {
    let mut fixture = match Fixture::new() {
        Ok(fixture) => fixture,
        Err(e) => {
            println!("{}", json!({"id": Value::Null, "ok": false, "error": e}));
            std::process::exit(2);
        }
    };
    let stdin = std::io::stdin();
    let mut out = std::io::stdout();
    let mut shutdown_cleanly = false;
    for line in stdin.lock().lines() {
        let line = match line {
            Ok(line) => line,
            Err(_) => break,
        };
        if line.trim().is_empty() {
            continue;
        }
        let cmd: Value = match serde_json::from_str(&line) {
            Ok(cmd) => cmd,
            Err(e) => {
                let _ = writeln!(
                    out,
                    "{}",
                    json!({"id": Value::Null, "ok": false, "error": format!("invalid command JSON: {e}")})
                );
                let _ = out.flush();
                continue;
            }
        };
        let is_shutdown = cmd.get("cmd").and_then(Value::as_str) == Some("shutdown");
        if let Some(response) = fixture.handle(&cmd).await {
            let _ = writeln!(out, "{response}");
            let _ = out.flush();
        }
        if is_shutdown {
            shutdown_cleanly = true;
            break;
        }
    }
    if !shutdown_cleanly {
        // EOF/crash path: still remove the isolated home and signal the
        // daemon we started so no test daemon leaks.
        fixture.cleanup();
    }
}

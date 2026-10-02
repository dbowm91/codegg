mod bridge;

use bridge::{ConnectionSnapshot, DesktopEvent, ProjectSummary};
use codegg_client::{
    connect_or_start_local_daemon, FrontendDescriptor, LocalDaemonOptions, LocalDaemonPaths,
    LocalSocketClient,
};
use codegg_protocol::{
    core::{CoreEvent, CoreRequest, CoreResponse, RequestEnvelope, PROTOCOL_VERSION},
    frames::{ClientCapabilities, ClientKind},
};
use std::{
    path::PathBuf,
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc,
    },
    time::Duration,
};
use tauri::{ipc::Channel, AppHandle, Manager, State};
use tokio::sync::Mutex;

const MAX_PROJECTS: usize = 50;

fn bound_projects(projects: impl IntoIterator<Item = ProjectSummary>) -> Vec<ProjectSummary> {
    projects.into_iter().take(MAX_PROJECTS).collect()
}

#[derive(Default)]
struct HostState {
    client: Mutex<Option<LocalSocketClient>>,
    daemon_id: Mutex<Option<String>>,
    generation: AtomicU64,
}

fn descriptor() -> FrontendDescriptor {
    FrontendDescriptor::new(
        "codegg-desktop",
        ClientKind::Gui,
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
            workspace_registration: false,
            project_catalog: true,
            session_projection: false,
        },
    )
}

fn daemon_executable(app: &AppHandle) -> Result<PathBuf, String> {
    if let Some(path) = std::env::var_os("CODEGG_DAEMON_EXECUTABLE").map(PathBuf::from) {
        return Ok(path);
    }
    let bundled = app
        .path()
        .resource_dir()
        .map_err(|e| e.to_string())?
        .join("codegg");
    if bundled.is_file() {
        return Ok(bundled);
    }
    Err("set CODEGG_DAEMON_EXECUTABLE to the existing codegg binary".into())
}

async fn get_snapshot(
    client: &LocalSocketClient,
    daemon_id: String,
) -> Result<ConnectionSnapshot, String> {
    let response = client
        .request(RequestEnvelope {
            protocol_version: PROTOCOL_VERSION,
            request_id: uuid::Uuid::new_v4().to_string(),
            payload: CoreRequest::SnapshotDaemon,
        })
        .await
        .map_err(|e| e.to_string())?;
    match response {
        CoreResponse::SnapshotDaemon {
            daemon_id: response_id,
            uptime_secs,
            active_sessions,
            ..
        } if response_id == daemon_id => Ok(ConnectionSnapshot {
            state: "connected".into(),
            daemon_id: Some(response_id),
            protocol_version: Some(PROTOCOL_VERSION),
            uptime_seconds: Some(uptime_secs),
            active_sessions: Some(active_sessions.len()),
            error: None,
        }),
        _ => Err("daemon returned an unexpected snapshot response".into()),
    }
}

#[tauri::command]
async fn desktop_connect(
    app: AppHandle,
    state: State<'_, Arc<HostState>>,
) -> Result<ConnectionSnapshot, String> {
    let generation = state.generation.fetch_add(1, Ordering::AcqRel) + 1;
    let paths = LocalDaemonPaths::resolve();
    paths.ensure_root().map_err(|e| e.to_string())?;
    let endpoint = paths.endpoint_uri();
    let frontend = descriptor();
    match LocalSocketClient::connect(&endpoint, frontend.clone()).await {
        Ok(client) => {
            let daemon_id = client.daemon_id().await.map_err(|e| e.to_string())?;
            let snapshot = get_snapshot(&client, daemon_id.clone()).await?;
            if state.generation.load(Ordering::Acquire) != generation {
                return Err("connection attempt was superseded".into());
            }
            *state.client.lock().await = Some(client);
            *state.daemon_id.lock().await = Some(daemon_id);
            return Ok(snapshot);
        }
        Err(error) if error.to_string().contains("protocol version mismatch") => {
            let message = error.to_string();
            return Ok(ConnectionSnapshot {
                state: "incompatible".into(),
                daemon_id: None,
                protocol_version: None,
                uptime_seconds: None,
                active_sessions: None,
                error: Some(message),
            });
        }
        Err(_) => {}
    }
    let outcome = connect_or_start_local_daemon(
        LocalDaemonOptions {
            endpoint,
            endpoint_argument: paths.socket_path_str(),
            lock_path: paths.lock_path,
            log_path: paths.log_path,
            executable: Some(daemon_executable(&app)?),
            autostart: true,
            startup_timeout: Duration::from_secs(20),
            poll_interval: Duration::from_millis(200),
        },
        frontend,
    )
    .await;
    let outcome = match outcome {
        Ok(outcome) => outcome,
        Err(error) => {
            let message = error.to_string();
            if message.contains("protocol version mismatch") {
                return Ok(ConnectionSnapshot {
                    state: "incompatible".into(),
                    daemon_id: None,
                    protocol_version: None,
                    uptime_seconds: None,
                    active_sessions: None,
                    error: Some(message),
                });
            }
            return Err(message);
        }
    };
    let snapshot = get_snapshot(&outcome.client, outcome.daemon_id.clone()).await?;
    if state.generation.load(Ordering::Acquire) != generation {
        return Err("connection attempt was superseded".into());
    }
    *state.client.lock().await = Some(outcome.client);
    *state.daemon_id.lock().await = Some(outcome.daemon_id);
    Ok(snapshot)
}

#[tauri::command]
async fn desktop_connection_snapshot(
    state: State<'_, Arc<HostState>>,
) -> Result<ConnectionSnapshot, String> {
    let client = state.client.lock().await.clone();
    let Some(client) = client else {
        return Ok(ConnectionSnapshot::disconnected(None));
    };
    let daemon_id = state.daemon_id.lock().await.clone().unwrap_or_default();
    Ok(get_snapshot(&client, daemon_id)
        .await
        .unwrap_or_else(|error| ConnectionSnapshot::disconnected(Some(error))))
}

#[tauri::command]
async fn desktop_project_list(
    state: State<'_, Arc<HostState>>,
) -> Result<Vec<ProjectSummary>, String> {
    let client = state
        .client
        .lock()
        .await
        .clone()
        .ok_or_else(|| "desktop is not connected".to_owned())?;
    let response = client
        .request(RequestEnvelope {
            protocol_version: PROTOCOL_VERSION,
            request_id: uuid::Uuid::new_v4().to_string(),
            payload: CoreRequest::ProjectList {
                include_archived: false,
                limit: MAX_PROJECTS,
            },
        })
        .await
        .map_err(|e| e.to_string())?;
    match response {
        CoreResponse::ProjectList { projects, .. } => {
            Ok(bound_projects(projects.into_iter().map(|p| {
                ProjectSummary {
                    project_id: p.project_id,
                    display_name: p.display_name,
                    lifecycle: p.lifecycle,
                }
            })))
        }
        _ => Err("daemon returned an unexpected project list response".into()),
    }
}

#[tauri::command]
async fn desktop_subscribe_events(
    state: State<'_, Arc<HostState>>,
    channel: Channel<DesktopEvent>,
) -> Result<(), String> {
    let client = state
        .client
        .lock()
        .await
        .clone()
        .ok_or_else(|| "desktop is not connected".to_owned())?;
    let mut events = client.subscribe();
    tauri::async_runtime::spawn(async move {
        while let Some(envelope) = events.recv().await {
            if matches!(
                envelope.payload,
                CoreEvent::ProjectRegistered { .. }
                    | CoreEvent::ProjectArchived { .. }
                    | CoreEvent::ProjectRestored { .. }
                    | CoreEvent::ProjectHealthChanged { .. }
            ) {
                if channel
                    .send(DesktopEvent {
                        version: 1,
                        event_seq: envelope.event_seq,
                        kind: "project_catalog_changed",
                    })
                    .is_err()
                {
                    break;
                }
            }
        }
    });
    Ok(())
}

#[tauri::command]
async fn desktop_disconnect(state: State<'_, Arc<HostState>>) -> Result<(), String> {
    state.generation.fetch_add(1, Ordering::AcqRel);
    *state.client.lock().await = None;
    *state.daemon_id.lock().await = None;
    Ok(())
}

pub fn run() {
    let state = Arc::new(HostState::default());
    tauri::Builder::default()
        .manage(Arc::clone(&state))
        .invoke_handler(tauri::generate_handler![
            desktop_connect,
            desktop_connection_snapshot,
            desktop_project_list,
            desktop_subscribe_events,
            desktop_disconnect
        ])
        .run(tauri::generate_context!())
        .expect("failed to run CodeGG desktop shell");
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn desktop_advertises_only_implemented_capabilities() {
        let d = descriptor();
        assert!(matches!(d.client_kind(), ClientKind::Gui));
        assert!(d.capabilities().project_catalog);
        assert!(!d.capabilities().session_projection && !d.capabilities().multi_session_view);
    }
    #[test]
    fn bridge_snapshot_has_stable_state_shape() {
        let json = serde_json::to_value(ConnectionSnapshot::disconnected(None)).unwrap();
        assert_eq!(json["state"], "disconnected");
        assert!(json.get("daemonId").is_some());
    }

    #[test]
    fn project_bridge_response_is_bounded() {
        let projects = (0..MAX_PROJECTS + 5).map(|index| ProjectSummary {
            project_id: format!("project-{index}"),
            display_name: format!("Project {index}"),
            lifecycle: "active".into(),
        });
        let projects = bound_projects(projects);
        assert_eq!(projects.len(), MAX_PROJECTS);
    }
}

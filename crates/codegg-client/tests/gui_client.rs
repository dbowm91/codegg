#![cfg(unix)]

use codegg_client::{
    connect_or_start_local_daemon, FrontendDescriptor, LocalDaemonOptions, LocalSocketClient,
};
use codegg_protocol::core::{CoreRequest, CoreResponse, RequestEnvelope, PROTOCOL_VERSION};
use codegg_protocol::frames::{
    ClientCapabilities, ClientKind, CoreFrame, ServerCapabilities, ServerHello,
};
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixListener;

fn gui_capabilities() -> ClientCapabilities {
    ClientCapabilities {
        visual_notifications: false,
        desktop_notifications: true,
        audio: false,
        tts: false,
        multi_session_view: true,
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
        session_projection: true,
    }
}

fn server_capabilities() -> ServerCapabilities {
    ServerCapabilities {
        event_replay: true,
        session_management: true,
        permission_routing: true,
        workspace_registration: true,
        workspace_snapshots: true,
        durable_jobs: true,
        durable_schedules: true,
        identity_aware_context: true,
        project_catalog: true,
        session_projection: true,
    }
}

#[tokio::test]
async fn gui_consumer_uses_native_client_without_root_or_tui_imports() {
    let socket = std::env::temp_dir().join(format!("codegg-client-{}.sock", uuid::Uuid::new_v4()));
    let listener = UnixListener::bind(&socket).expect("bind fake daemon");
    let server = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.expect("accept GUI");
        let (read, mut write) = stream.into_split();
        let mut reader = BufReader::new(read);
        let mut line = String::new();
        reader.read_line(&mut line).await.expect("read hello");
        let CoreFrame::ClientHello(hello) =
            serde_json::from_str(line.trim()).expect("decode hello")
        else {
            panic!("expected ClientHello");
        };
        assert_eq!(hello.client_name, "codegg-gui-test");
        assert!(matches!(hello.client_kind, ClientKind::Gui));
        assert!(hello.capabilities.multi_session_view);
        let response = CoreFrame::ServerHello(ServerHello {
            daemon_id: "fake-daemon".into(),
            protocol_version: PROTOCOL_VERSION,
            server_capabilities: server_capabilities(),
            client_id: "gui-client-id".into(),
        });
        write
            .write_all(format!("{}\n", serde_json::to_string(&response).unwrap()).as_bytes())
            .await
            .expect("send ServerHello");
        write.flush().await.expect("flush hello");

        let mut request_ids = Vec::new();
        while request_ids.len() < 2 {
            line.clear();
            reader
                .read_line(&mut line)
                .await
                .expect("read client frame");
            if let CoreFrame::Request(request) =
                serde_json::from_str(line.trim()).expect("decode client frame")
            {
                assert!(matches!(request.payload, CoreRequest::SnapshotDaemon));
                request_ids.push(request.request_id);
            }
        }
        for (request_id, event_seq) in request_ids.into_iter().zip([2, 1]) {
            let response = CoreFrame::Response {
                request_id,
                response: Box::new(CoreResponse::SnapshotDaemon {
                    event_seq,
                    daemon_id: format!("fake-daemon-{event_seq}"),
                    uptime_secs: 1,
                    active_sessions: vec![],
                    connected_clients: vec![],
                    scheduler_snapshot: None,
                }),
            };
            write
                .write_all(format!("{}\n", serde_json::to_string(&response).unwrap()).as_bytes())
                .await
                .expect("send response");
            write.flush().await.expect("flush response");
        }
    });

    let endpoint = format!("unix://{}", socket.display());
    let client = LocalSocketClient::connect(
        endpoint,
        FrontendDescriptor::new("codegg-gui-test", ClientKind::Gui, gui_capabilities()),
    )
    .await
    .expect("connect GUI client");
    assert_eq!(client.daemon_id().await.unwrap(), "fake-daemon");
    assert_eq!(client.client_id().await.as_deref(), Some("gui-client-id"));
    let request_a = client.request(RequestEnvelope {
        protocol_version: PROTOCOL_VERSION,
        request_id: "gui-snapshot-a".into(),
        payload: CoreRequest::SnapshotDaemon,
    });
    let request_b = client.request(RequestEnvelope {
        protocol_version: PROTOCOL_VERSION,
        request_id: "gui-snapshot-b".into(),
        payload: CoreRequest::SnapshotDaemon,
    });
    let (response_a, response_b) = tokio::join!(request_a, request_b);
    let CoreResponse::SnapshotDaemon {
        event_seq: seq_a, ..
    } = response_a.expect("response A")
    else {
        panic!("request A returned another response variant");
    };
    let CoreResponse::SnapshotDaemon {
        event_seq: seq_b, ..
    } = response_b.expect("response B")
    else {
        panic!("request B returned another response variant");
    };
    assert_eq!((seq_a, seq_b), (2, 1));
    drop(client);
    server.await.expect("fake daemon task");
    let _ = std::fs::remove_file(socket);
}

#[tokio::test]
async fn reconnect_negotiates_a_fresh_client_identity() {
    let socket =
        std::env::temp_dir().join(format!("codegg-reconnect-{}.sock", uuid::Uuid::new_v4()));
    let listener = UnixListener::bind(&socket).expect("bind fake daemon");
    let server = tokio::spawn(async move {
        let mut writers = Vec::new();
        for ordinal in 1..=2 {
            let (stream, _) = listener.accept().await.expect("accept client connection");
            let (read, mut write) = stream.into_split();
            let mut reader = BufReader::new(read);
            let mut line = String::new();
            reader.read_line(&mut line).await.expect("read ClientHello");
            let _: CoreFrame = serde_json::from_str(line.trim()).expect("decode ClientHello");
            let hello = CoreFrame::ServerHello(ServerHello {
                daemon_id: "reconnect-daemon".into(),
                protocol_version: PROTOCOL_VERSION,
                server_capabilities: server_capabilities(),
                client_id: format!("client-{ordinal}"),
            });
            write
                .write_all(format!("{}\n", serde_json::to_string(&hello).unwrap()).as_bytes())
                .await
                .expect("send ServerHello");
            write.flush().await.expect("flush ServerHello");
            line.clear();
            reader
                .read_line(&mut line)
                .await
                .expect("read default Subscribe");
            assert!(matches!(
                serde_json::from_str::<CoreFrame>(line.trim()).expect("decode Subscribe"),
                CoreFrame::Subscribe { .. }
            ));
            writers.push(write);
        }
        writers
    });

    let client = LocalSocketClient::connect(
        format!("unix://{}", socket.display()),
        FrontendDescriptor::new("codegg-gui-test", ClientKind::Gui, gui_capabilities()),
    )
    .await
    .expect("connect first generation");
    let first_id = client.client_id().await.expect("first client id");
    client.reconnect().await.expect("reconnect client");
    let second_id = client.client_id().await.expect("second client id");
    assert_eq!(first_id, "client-1");
    assert_eq!(second_id, "client-2");
    drop(client);
    let _writers = server.await.expect("fake daemon task");
    let _ = std::fs::remove_file(socket);
}

#[tokio::test]
async fn peer_death_releases_pending_request_with_error() {
    let socket =
        std::env::temp_dir().join(format!("codegg-peer-death-{}.sock", uuid::Uuid::new_v4()));
    let listener = UnixListener::bind(&socket).expect("bind fake daemon");
    let server = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.expect("accept client");
        let (read, mut write) = stream.into_split();
        let mut reader = BufReader::new(read);
        let mut line = String::new();
        reader.read_line(&mut line).await.expect("read ClientHello");
        let hello = CoreFrame::ServerHello(ServerHello {
            daemon_id: "peer-death-daemon".into(),
            protocol_version: PROTOCOL_VERSION,
            server_capabilities: server_capabilities(),
            client_id: "peer-death-client".into(),
        });
        write
            .write_all(format!("{}\n", serde_json::to_string(&hello).unwrap()).as_bytes())
            .await
            .expect("send ServerHello");
        write.flush().await.expect("flush ServerHello");
        line.clear();
        reader
            .read_line(&mut line)
            .await
            .expect("read initial subscription");
        line.clear();
        reader
            .read_line(&mut line)
            .await
            .expect("read pending request");
        assert!(matches!(
            serde_json::from_str::<CoreFrame>(line.trim()).expect("decode pending request"),
            CoreFrame::Request(_)
        ));
        drop(write);
    });

    let client = LocalSocketClient::connect(
        format!("unix://{}", socket.display()),
        FrontendDescriptor::new("codegg-gui-test", ClientKind::Gui, gui_capabilities()),
    )
    .await
    .expect("connect client");
    let result = tokio::time::timeout(
        Duration::from_secs(2),
        client.request(RequestEnvelope {
            protocol_version: PROTOCOL_VERSION,
            request_id: "peer-death-request".into(),
            payload: CoreRequest::SnapshotDaemon,
        }),
    )
    .await
    .expect("peer closure resolves request waiter");
    assert!(result.is_err(), "peer death must fail the pending request");
    drop(client);
    server.await.expect("peer-death fixture");
    let _ = std::fs::remove_file(socket);
}

#[tokio::test]
async fn protocol_version_mismatch_fails_the_handshake() {
    let socket = std::env::temp_dir().join(format!(
        "codegg-version-mismatch-{}.sock",
        uuid::Uuid::new_v4()
    ));
    let listener = UnixListener::bind(&socket).expect("bind fake daemon");
    let server = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.expect("accept client");
        let (read, mut write) = stream.into_split();
        let mut reader = BufReader::new(read);
        let mut line = String::new();
        reader.read_line(&mut line).await.expect("read ClientHello");
        let hello = CoreFrame::ServerHello(ServerHello {
            daemon_id: "wrong-version-daemon".into(),
            protocol_version: PROTOCOL_VERSION + 1,
            server_capabilities: server_capabilities(),
            client_id: "wrong-version-client".into(),
        });
        write
            .write_all(format!("{}\n", serde_json::to_string(&hello).unwrap()).as_bytes())
            .await
            .expect("send mismatched ServerHello");
        write.flush().await.expect("flush mismatched ServerHello");
    });

    let result = LocalSocketClient::connect(
        format!("unix://{}", socket.display()),
        FrontendDescriptor::new("codegg-gui-test", ClientKind::Gui, gui_capabilities()),
    )
    .await;
    assert!(
        matches!(result, Err(codegg_client::ClientError::Handshake(message)) if message.contains("protocol version mismatch")),
        "a protocol mismatch must be an explicit handshake error"
    );
    server.await.expect("mismatched protocol fixture");
    let _ = std::fs::remove_file(socket);
}

#[tokio::test]
async fn gui_disconnect_does_not_close_a_concurrent_tui_client() {
    let socket = std::env::temp_dir().join(format!("codegg-pair-{}.sock", uuid::Uuid::new_v4()));
    let listener = UnixListener::bind(&socket).expect("bind fake daemon");
    let server = tokio::spawn(async move {
        let mut connections = Vec::new();
        for (ordinal, expected_kind) in [ClientKind::Tui, ClientKind::Gui].into_iter().enumerate() {
            let (stream, _) = listener.accept().await.expect("accept frontend");
            let (read, mut write) = stream.into_split();
            let mut reader = BufReader::new(read);
            let mut line = String::new();
            reader.read_line(&mut line).await.expect("read ClientHello");
            let CoreFrame::ClientHello(client_hello) =
                serde_json::from_str(line.trim()).expect("decode ClientHello")
            else {
                panic!("expected ClientHello");
            };
            assert!(
                std::mem::discriminant(&client_hello.client_kind)
                    == std::mem::discriminant(&expected_kind)
            );
            let hello = CoreFrame::ServerHello(ServerHello {
                daemon_id: "shared-daemon".into(),
                protocol_version: PROTOCOL_VERSION,
                server_capabilities: server_capabilities(),
                client_id: format!("shared-client-{ordinal}"),
            });
            write
                .write_all(format!("{}\n", serde_json::to_string(&hello).unwrap()).as_bytes())
                .await
                .expect("send ServerHello");
            write.flush().await.expect("flush ServerHello");
            line.clear();
            reader.read_line(&mut line).await.expect("read Subscribe");
            assert!(matches!(
                serde_json::from_str::<CoreFrame>(line.trim()).expect("decode Subscribe"),
                CoreFrame::Subscribe { .. }
            ));
            connections.push((reader, write));
        }

        let (reader, write) = &mut connections[0];
        let mut line = String::new();
        reader
            .read_line(&mut line)
            .await
            .expect("read surviving TUI request");
        let CoreFrame::Request(request) =
            serde_json::from_str(line.trim()).expect("decode TUI request")
        else {
            panic!("expected request");
        };
        let response = CoreFrame::Response {
            request_id: request.request_id,
            response: Box::new(CoreResponse::SnapshotDaemon {
                event_seq: 0,
                daemon_id: "shared-daemon".into(),
                uptime_secs: 1,
                active_sessions: vec![],
                connected_clients: vec![],
                scheduler_snapshot: None,
            }),
        };
        write
            .write_all(format!("{}\n", serde_json::to_string(&response).unwrap()).as_bytes())
            .await
            .expect("respond to surviving TUI");
        write.flush().await.expect("flush TUI response");
        connections
    });

    let endpoint = format!("unix://{}", socket.display());
    let mut tui_caps = gui_capabilities();
    tui_caps.desktop_notifications = false;
    let tui = LocalSocketClient::connect(
        &endpoint,
        FrontendDescriptor::new("codegg-tui-test", ClientKind::Tui, tui_caps),
    )
    .await
    .expect("connect TUI");
    let gui = LocalSocketClient::connect(
        endpoint,
        FrontendDescriptor::new("codegg-gui-test", ClientKind::Gui, gui_capabilities()),
    )
    .await
    .expect("connect GUI");
    assert_ne!(tui.client_id().await, gui.client_id().await);
    drop(gui);
    let response = tui
        .request(RequestEnvelope {
            protocol_version: PROTOCOL_VERSION,
            request_id: "tui-survives-gui-drop".into(),
            payload: CoreRequest::SnapshotDaemon,
        })
        .await
        .expect("TUI request after GUI disconnect");
    assert!(matches!(response, CoreResponse::SnapshotDaemon { .. }));
    drop(tui);
    let _connections = server.await.expect("fake daemon task");
    let _ = std::fs::remove_file(socket);
}

#[tokio::test]
async fn connect_or_start_reuses_only_a_verified_existing_daemon() {
    let root = std::env::temp_dir().join(format!("codegg-connect-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&root).expect("create fixture directory");
    let socket = root.join("core.sock");
    let listener = UnixListener::bind(&socket).expect("bind fake daemon");
    let server = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.expect("accept client");
        let (read, mut write) = stream.into_split();
        let mut reader = BufReader::new(read);
        let mut line = String::new();
        reader.read_line(&mut line).await.expect("read ClientHello");
        let server_hello = CoreFrame::ServerHello(ServerHello {
            daemon_id: "reused-daemon".into(),
            protocol_version: PROTOCOL_VERSION,
            server_capabilities: server_capabilities(),
            client_id: "reuse-client".into(),
        });
        write
            .write_all(format!("{}\n", serde_json::to_string(&server_hello).unwrap()).as_bytes())
            .await
            .expect("send ServerHello");
        write.flush().await.expect("flush ServerHello");
        line.clear();
        reader.read_line(&mut line).await.expect("read Subscribe");
        line.clear();
        reader
            .read_line(&mut line)
            .await
            .expect("read identity probe");
        let CoreFrame::Request(request) = serde_json::from_str(line.trim()).expect("decode probe")
        else {
            panic!("expected snapshot probe");
        };
        let response = CoreFrame::Response {
            request_id: request.request_id,
            response: Box::new(CoreResponse::SnapshotDaemon {
                event_seq: 0,
                daemon_id: "reused-daemon".into(),
                uptime_secs: 10,
                active_sessions: vec![],
                connected_clients: vec![],
                scheduler_snapshot: None,
            }),
        };
        write
            .write_all(format!("{}\n", serde_json::to_string(&response).unwrap()).as_bytes())
            .await
            .expect("send snapshot");
        write.flush().await.expect("flush snapshot");
    });

    let endpoint = format!("unix://{}", socket.display());
    let result = connect_or_start_local_daemon(
        LocalDaemonOptions {
            endpoint: endpoint.clone(),
            endpoint_argument: socket.to_string_lossy().into_owned(),
            lock_path: root.join("daemon.lock"),
            log_path: root.join("daemon.log"),
            executable: None,
            autostart: false,
            startup_timeout: Duration::from_secs(2),
            poll_interval: Duration::from_millis(10),
        },
        FrontendDescriptor::new("codegg-gui-test", ClientKind::Gui, gui_capabilities()),
    )
    .await
    .expect("reuse healthy daemon");
    assert_eq!(result.daemon_id, "reused-daemon");
    assert_eq!(result.endpoint, endpoint);
    assert_eq!(result.started_pid, None);
    drop(result.client);
    server.await.expect("fake daemon task");
    let _ = std::fs::remove_dir_all(root);
}

#[tokio::test]
async fn unresponsive_endpoint_does_not_consume_the_daemon_startup_budget() {
    let root = std::env::temp_dir().join(format!("codegg-stale-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&root).expect("create fixture directory");
    let socket = root.join("core.sock");
    let listener = UnixListener::bind(&socket).expect("bind stale endpoint");
    let server = tokio::spawn(async move {
        let (_stream, _) = listener
            .accept()
            .await
            .expect("accept stale endpoint probe");
        tokio::time::sleep(Duration::from_secs(2)).await;
    });

    let started = tokio::time::Instant::now();
    let result = connect_or_start_local_daemon(
        LocalDaemonOptions {
            endpoint: format!("unix://{}", socket.display()),
            endpoint_argument: socket.to_string_lossy().into_owned(),
            lock_path: root.join("daemon.lock"),
            log_path: root.join("daemon.log"),
            executable: None,
            autostart: false,
            startup_timeout: Duration::from_secs(5),
            poll_interval: Duration::from_millis(10),
        },
        FrontendDescriptor::new("codegg-gui-test", ClientKind::Gui, gui_capabilities()),
    )
    .await;
    assert!(
        result.is_err(),
        "an endpoint without ServerHello is not healthy"
    );
    assert!(
        started.elapsed() < Duration::from_secs(3),
        "one stale endpoint must not consume the full startup deadline"
    );
    server.await.expect("stale endpoint fixture");
    let _ = std::fs::remove_dir_all(root);
}

#[tokio::test]
async fn child_exit_before_readiness_returns_a_typed_startup_error() {
    use std::os::unix::fs::PermissionsExt;

    let root = std::env::temp_dir().join(format!("codegg-bad-child-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&root).expect("create fixture directory");
    let socket = root.join("core.sock");
    let executable = root.join("daemon-child");
    std::fs::write(&executable, "#!/bin/sh\nexit 17\n").expect("write failing child");
    let mut permissions = std::fs::metadata(&executable)
        .expect("read child permissions")
        .permissions();
    permissions.set_mode(0o700);
    std::fs::set_permissions(&executable, permissions).expect("make child executable");

    let result = connect_or_start_local_daemon(
        LocalDaemonOptions {
            endpoint: format!("unix://{}", socket.display()),
            endpoint_argument: socket.to_string_lossy().into_owned(),
            lock_path: root.join("daemon.lock"),
            log_path: root.join("daemon.log"),
            executable: Some(executable),
            autostart: true,
            startup_timeout: Duration::from_secs(2),
            poll_interval: Duration::from_millis(10),
        },
        FrontendDescriptor::new("codegg-gui-test", ClientKind::Gui, gui_capabilities()),
    )
    .await;
    assert!(
        matches!(result, Err(codegg_client::ClientError::ChildExited(status)) if status.contains("17")),
        "early child exit should retain its actionable status"
    );
    let _ = std::fs::remove_dir_all(root);
}

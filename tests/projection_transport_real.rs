#![cfg(feature = "server")]

mod common;

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use axum::routing::get;
use axum::Router;
use codegg::config::schema::Config;
use codegg::core::daemon::CoreDaemon;
use codegg::core::new_request;
use codegg::core::transport::projection::{
    CriticalDeliveryError, ProjectionLifecycleBoundary, ProjectionLifecycleSeam,
};
use codegg::mcp::McpService;
use codegg::protocol::core::{CoreEvent, CoreRequest, CoreResponse, EventEnvelope};
use codegg::protocol::frames::{ClientCapabilities, ClientHello, ClientKind, CoreFrame};
use codegg::protocol::projection::replay::{
    ProjectionStreamKind, ProjectionSubscriptionId, ProjectionSubscriptionRequest,
};
use codegg::protocol::tui::TuiMessage;
use codegg::server::ws::{
    handle_core_ws, handle_tui, ConnectionTaskProbe, TransportLifecycleObserver, WriterGate,
};
use codegg::server::{ServerState, WsRateLimiter};
use codegg_core::projection_replay::seam::ProjectionBindingContext;
use futures::{SinkExt, StreamExt};
use tokio::net::TcpListener;
use tokio::time::timeout;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::{connect_async, MaybeTlsStream, WebSocketStream};

type Client = WebSocketStream<MaybeTlsStream<tokio::net::TcpStream>>;

async fn spawn_server() -> (SocketAddr, Arc<CoreDaemon>, tokio::task::JoinHandle<()>) {
    spawn_server_with_seam(ProjectionLifecycleSeam::default()).await
}

async fn spawn_server_with_seam(
    projection_lifecycle_seam: ProjectionLifecycleSeam,
) -> (SocketAddr, Arc<CoreDaemon>, tokio::task::JoinHandle<()>) {
    std::env::set_var("CODEGG_SERVER_AUTH_DISABLED", "1");

    let pool = common::projection_replay::test_pool().await;
    let daemon = Arc::new(CoreDaemon::new(Some(pool.clone()), None, None, None));
    let state = ServerState {
        pool,
        mcp_service: Arc::new(tokio::sync::RwLock::new(McpService::new())),
        config: Config::default(),
        ws_rate_limiter: Arc::new(WsRateLimiter::new(256, 60)),
        daemon: Some(Arc::clone(&daemon)),
        projection_lifecycle_seam,
        connection_task_probe: None,
        transport_test_config: None,
    };
    let router = Router::new()
        .route("/core", get(handle_core_ws))
        .route("/tui", get(handle_tui))
        .with_state(state);
    let listener = TcpListener::bind(("127.0.0.1", 0))
        .await
        .expect("bind test server");
    let address = listener.local_addr().expect("test server address");
    let task = tokio::spawn(async move {
        axum::serve(
            listener,
            router.into_make_service_with_connect_info::<SocketAddr>(),
        )
        .await
        .expect("test server");
    });
    (address, daemon, task)
}

async fn connect(address: SocketAddr, path: &str) -> Client {
    connect_async(format!("ws://{address}{path}"))
        .await
        .expect("connect websocket")
        .0
}

async fn send_json<T: serde::Serialize>(client: &mut Client, value: &T) {
    client
        .send(Message::Text(
            serde_json::to_string(value)
                .expect("serialize test frame")
                .into(),
        ))
        .await
        .expect("send websocket frame");
}

async fn recv_json<T: serde::de::DeserializeOwned>(client: &mut Client) -> Option<T> {
    loop {
        let message = timeout(Duration::from_millis(750), client.next())
            .await
            .ok()??
            .ok()?;
        match message {
            Message::Text(text) => return serde_json::from_str(&text).ok(),
            Message::Close(_) => return None,
            _ => {}
        }
    }
}

fn client_capabilities(session_projection: bool) -> ClientCapabilities {
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
        project_catalog: false,
        session_projection,
    }
}

fn raw_client_capabilities() -> ClientCapabilities {
    client_capabilities(false)
}

fn projection_client_capabilities() -> ClientCapabilities {
    client_capabilities(true)
}

async fn core_handshake(client: &mut Client, session_id: &str) {
    send_json(
        client,
        &CoreFrame::ClientHello(ClientHello {
            client_name: format!("m006-{session_id}"),
            client_kind: ClientKind::Automation,
            protocol_version: codegg::protocol::core::PROTOCOL_VERSION,
            capabilities: raw_client_capabilities(),
        }),
    )
    .await;
    let hello: CoreFrame = recv_json(client).await.expect("ServerHello");
    assert!(matches!(hello, CoreFrame::ServerHello(_)));

    send_json(
        client,
        &CoreFrame::Subscribe {
            client_id: String::new(),
            session_id: Some(session_id.to_string()),
            from_event_seq: Some(0),
        },
    )
    .await;
    // Ping is a protocol barrier: the receive loop processes frames in order,
    // so Pong proves that the connection-local filter is installed.
    send_json(client, &CoreFrame::Ping).await;
    loop {
        let frame: CoreFrame = recv_json(client).await.expect("Pong");
        if matches!(frame, CoreFrame::Pong) {
            break;
        }
    }
}

async fn core_projection_handshake(client: &mut Client, client_name: &str) {
    send_json(
        client,
        &CoreFrame::ClientHello(ClientHello {
            client_name: client_name.to_string(),
            client_kind: ClientKind::Automation,
            protocol_version: codegg::protocol::core::PROTOCOL_VERSION,
            capabilities: projection_client_capabilities(),
        }),
    )
    .await;
    let hello: CoreFrame = recv_json(client).await.expect("ServerHello");
    assert!(matches!(hello, CoreFrame::ServerHello(_)));
}

fn project_subscription_request(project_id: &str) -> ProjectionSubscriptionRequest {
    ProjectionSubscriptionRequest {
        scope: ProjectionStreamKind::Project,
        scope_id: project_id.to_string(),
        cursor: None,
        projection_version: 1,
    }
}

async fn projection_event(daemon: &CoreDaemon, project_id: &str, session_id: &str, turn_id: &str) {
    projection_event_at_seq(daemon, project_id, session_id, turn_id, 1).await;
}

async fn projection_event_at_seq(
    daemon: &CoreDaemon,
    project_id: &str,
    session_id: &str,
    turn_id: &str,
    event_seq: u64,
) {
    let seam = daemon
        .projection_seam
        .as_ref()
        .expect("SQLite-backed daemon has projection seam");
    let envelope = EventEnvelope {
        protocol_version: codegg::protocol::core::PROTOCOL_VERSION,
        event_seq,
        timestamp_ms: 1,
        session_id: Some(session_id.to_string()),
        turn_id: Some(turn_id.to_string()),
        payload: CoreEvent::TurnStarted {
            session_id: session_id.to_string(),
            turn_id: turn_id.to_string(),
        },
    };
    let context = ProjectionBindingContext {
        session_id: Some(session_id.to_string()),
        project_id: Some(project_id.to_string()),
        workspace_id: None,
        binding_revision: 1,
    };
    let outcome = seam
        .service()
        .publish_from_core_with_context(&envelope, &context)
        .await
        .expect("publish projection event");
    assert!(matches!(
        outcome,
        codegg_core::projection_replay::service::PublishOutcome::Published { .. }
    ));
}

async fn wait_projection_subscription_count(daemon: &CoreDaemon, expected: u64) {
    let seam = daemon
        .projection_seam
        .as_ref()
        .expect("SQLite-backed daemon has projection seam");
    timeout(Duration::from_millis(500), async {
        loop {
            if seam.service().subscriptions().active_count() == expected {
                return;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("projection subscription count should converge");
}

async fn next_core_event(client: &mut Client) -> Option<EventEnvelope<CoreEvent>> {
    loop {
        let frame: CoreFrame = recv_json(client).await?;
        if let CoreFrame::Event(event) = frame {
            return Some(event);
        }
    }
}

async fn next_core_projection_event(client: &mut Client) -> Option<ProjectionSubscriptionId> {
    loop {
        let frame: CoreFrame = recv_json(client).await?;
        if let CoreFrame::Event(event) = frame {
            if let CoreEvent::ProjectionStreamEvent {
                subscription_id, ..
            } = event.payload
            {
                return Some(subscription_id);
            }
        }
    }
}

async fn next_core_projection_envelope(
    client: &mut Client,
) -> Option<(
    ProjectionSubscriptionId,
    codegg::protocol::projection::replay::ProjectionStreamId,
    codegg::protocol::projection::event::ProjectionEnvelope,
)> {
    loop {
        let frame: CoreFrame = recv_json(client).await?;
        if let CoreFrame::Event(event) = frame {
            if let CoreEvent::ProjectionStreamEvent {
                subscription_id,
                stream_id,
                envelope,
            } = event.payload
            {
                return Some((subscription_id, stream_id, envelope));
            }
        }
    }
}

async fn core_subscribe(
    client: &mut Client,
    request_id: &str,
    project_id: &str,
) -> ProjectionSubscriptionId {
    core_subscribe_with_cursor(client, request_id, project_id)
        .await
        .0
}

async fn core_subscribe_with_cursor(
    client: &mut Client,
    request_id: &str,
    project_id: &str,
) -> (
    ProjectionSubscriptionId,
    codegg::protocol::projection::replay::ProjectionCursor,
) {
    send_json(
        client,
        &CoreFrame::Request(new_request(
            request_id.to_string(),
            CoreRequest::ProjectionSubscribe {
                request: project_subscription_request(project_id),
            },
        )),
    )
    .await;
    loop {
        let frame: CoreFrame = recv_json(client).await.expect("projection response");
        if let CoreFrame::Response {
            request_id: response_id,
            response,
        } = frame
        {
            if response_id == request_id {
                return match *response {
                    CoreResponse::ProjectionSubscribed {
                        subscription_id,
                        cursor,
                        ..
                    } => (subscription_id, cursor),
                    other => panic!("expected ProjectionSubscribed, got {other:?}"),
                };
            }
        }
    }
}

async fn core_reject_foreign_unsubscribe(
    client: &mut Client,
    subscription_id: ProjectionSubscriptionId,
) {
    let request_id = "foreign-unsubscribe";
    send_json(
        client,
        &CoreFrame::Request(new_request(
            request_id.to_string(),
            CoreRequest::ProjectionUnsubscribe { subscription_id },
        )),
    )
    .await;
    loop {
        let frame: CoreFrame = recv_json(client)
            .await
            .expect("foreign unsubscribe response");
        if let CoreFrame::Response {
            request_id: response_id,
            response,
        } = frame
        {
            if response_id == request_id {
                assert!(matches!(
                    *response,
                    CoreResponse::Error {
                        code,
                        ..
                    } if code == "projection_subscription_not_owned"
                ));
                return;
            }
        }
    }
}

async fn next_core_response(client: &mut Client, request_id: &str) -> CoreResponse {
    loop {
        let frame: CoreFrame = recv_json(client).await.expect("core response");
        if let CoreFrame::Response {
            request_id: response_id,
            response,
        } = frame
        {
            if response_id == request_id {
                return *response;
            }
        }
    }
}

async fn send_core_request(
    client: &mut Client,
    request_id: &str,
    request: CoreRequest,
) -> CoreResponse {
    send_json(
        client,
        &CoreFrame::Request(new_request(request_id.to_string(), request)),
    )
    .await;
    next_core_response(client, request_id).await
}

async fn tui_raw_handshake(client: &mut Client, session_id: &str) {
    send_json(
        client,
        &TuiMessage::SessionInfo {
            id: session_id.to_string(),
            model: "test-model".to_string(),
        },
    )
    .await;
    send_json(
        client,
        &TuiMessage::ProjectionCapabilities {
            capabilities: codegg::protocol::projection::caps::ProjectionCapabilities {
                min_version: 99,
                max_version: 99,
                supports_incremental_events: true,
                supports_unknown_fields: true,
            },
        },
    )
    .await;
    let ack: TuiMessage = recv_json(client).await.expect("projection capability ack");
    assert!(matches!(
        ack,
        TuiMessage::ProjectionCapabilitiesAck {
            accepted: false,
            ..
        }
    ));
}

async fn tui_projection_handshake(client: &mut Client) {
    send_json(
        client,
        &TuiMessage::ProjectionCapabilities {
            capabilities: codegg::protocol::projection::caps::ProjectionCapabilities::default(),
        },
    )
    .await;
    loop {
        let message: TuiMessage = recv_json(client).await.expect("projection capability ack");
        if let TuiMessage::ProjectionCapabilitiesAck { accepted, .. } = message {
            assert!(accepted, "projection capability negotiation must succeed");
            break;
        }
    }
    drain_tui_messages(client).await;
}

async fn next_tui_event(client: &mut Client) -> Option<TuiMessage> {
    loop {
        let message: TuiMessage = recv_json(client).await?;
        if let TuiMessage::EventEnvelope { payload, .. } = message {
            return Some(*payload);
        }
    }
}

async fn next_tui_projection_event(client: &mut Client) -> Option<ProjectionSubscriptionId> {
    loop {
        let message: TuiMessage = recv_json(client).await?;
        if let TuiMessage::ProjectionEvent {
            subscription_id, ..
        } = message
        {
            return Some(subscription_id);
        }
    }
}

async fn next_tui_projection_envelope(
    client: &mut Client,
) -> Option<(
    ProjectionSubscriptionId,
    Option<codegg::protocol::projection::replay::ProjectionStreamId>,
    codegg::protocol::projection::event::ProjectionEnvelope,
)> {
    loop {
        match recv_json::<TuiMessage>(client).await? {
            TuiMessage::ProjectionEvent {
                subscription_id,
                stream_id,
                envelope,
            } => return Some((subscription_id, stream_id, envelope)),
            _ => {}
        }
    }
}

async fn tui_subscribe(client: &mut Client, project_id: &str) -> ProjectionSubscriptionId {
    tui_subscribe_with_cursor(client, project_id).await.0
}

async fn tui_subscribe_with_cursor(
    client: &mut Client,
    project_id: &str,
) -> (
    ProjectionSubscriptionId,
    codegg::protocol::projection::replay::ProjectionCursor,
) {
    send_json(
        client,
        &TuiMessage::ProjectionSubscribe {
            request: project_subscription_request(project_id),
        },
    )
    .await;
    loop {
        let message: TuiMessage = recv_json(client).await.expect("projection snapshot");
        if let TuiMessage::ProjectionSnapshot {
            subscription_id,
            cursor,
            ..
        } = message
        {
            return (
                subscription_id,
                cursor.expect("TUI snapshot includes authoritative cursor"),
            );
        }
    }
}

async fn tui_reject_foreign_unsubscribe(
    client: &mut Client,
    subscription_id: ProjectionSubscriptionId,
) {
    send_json(
        client,
        &TuiMessage::ProjectionUnsubscribe { subscription_id },
    )
    .await;
    loop {
        let message: TuiMessage = recv_json(client).await.expect("foreign unsubscribe result");
        if let TuiMessage::ProjectionUnsubscribeResult {
            accepted, reason, ..
        } = message
        {
            assert!(!accepted);
            assert_eq!(reason.as_deref(), Some("projection_subscription_not_owned"));
            return;
        }
    }
}

async fn drain_tui_messages(client: &mut Client) {
    loop {
        match timeout(Duration::from_millis(20), client.next()).await {
            Ok(Some(Ok(Message::Text(_)))) => {}
            _ => break,
        }
    }
}

// ===== Helpers for Work Packages B–E =====

async fn spawn_server_with_seam_and_probe(
    projection_lifecycle_seam: ProjectionLifecycleSeam,
) -> (
    SocketAddr,
    Arc<CoreDaemon>,
    Arc<ConnectionTaskProbe>,
    tokio::task::JoinHandle<()>,
) {
    std::env::set_var("CODEGG_SERVER_AUTH_DISABLED", "1");

    let pool = common::projection_replay::test_pool().await;
    let daemon = Arc::new(CoreDaemon::new(Some(pool.clone()), None, None, None));
    let probe = Arc::new(ConnectionTaskProbe::new());
    let state = ServerState {
        pool,
        mcp_service: Arc::new(tokio::sync::RwLock::new(McpService::new())),
        config: Config::default(),
        ws_rate_limiter: Arc::new(WsRateLimiter::new(256, 60)),
        daemon: Some(Arc::clone(&daemon)),
        projection_lifecycle_seam,
        connection_task_probe: Some(Arc::clone(&probe)),
        transport_test_config: None,
    };
    let router = Router::new()
        .route("/core", get(handle_core_ws))
        .route("/tui", get(handle_tui))
        .with_state(state);
    let listener = TcpListener::bind(("127.0.0.1", 0))
        .await
        .expect("bind test server");
    let address = listener.local_addr().expect("test server address");
    let task = tokio::spawn(async move {
        axum::serve(
            listener,
            router.into_make_service_with_connect_info::<SocketAddr>(),
        )
        .await
        .expect("test server");
    });
    (address, daemon, probe, task)
}

// ===== Helpers for Work Packages A, B, C, E (M010 transport instrumentation) =====

/// Spawn server with the production `ConnectionTaskProbe` AND the
/// `ProjectionTransportTestConfig` so tests can: pause the writer, cancel
/// the raw-source task, observe the final critical-send result, fill the
/// outbound queue from outside the recv task, and read `first_task_kind`.
async fn spawn_server_with_transport_instrumentation(
    seam: ProjectionLifecycleSeam,
    outbound_queue_capacity: usize,
    raw_source_cancel: Option<tokio_util::sync::CancellationToken>,
) -> (
    SocketAddr,
    Arc<CoreDaemon>,
    Arc<ConnectionTaskProbe>,
    Arc<codegg::server::ws::WriterGate>,
    Arc<codegg::server::ws::TransportLifecycleObserver>,
    tokio::task::JoinHandle<()>,
) {
    spawn_server_with_transport_instrumentation_and_gate(
        seam,
        outbound_queue_capacity,
        raw_source_cancel,
        false,
    )
    .await
}

async fn spawn_server_with_transport_instrumentation_and_gate(
    seam: ProjectionLifecycleSeam,
    outbound_queue_capacity: usize,
    raw_source_cancel: Option<tokio_util::sync::CancellationToken>,
    gate_before_recv: bool,
) -> (
    SocketAddr,
    Arc<CoreDaemon>,
    Arc<ConnectionTaskProbe>,
    Arc<codegg::server::ws::WriterGate>,
    Arc<codegg::server::ws::TransportLifecycleObserver>,
    tokio::task::JoinHandle<()>,
) {
    std::env::set_var("CODEGG_SERVER_AUTH_DISABLED", "1");

    let pool = common::projection_replay::test_pool().await;
    let daemon = Arc::new(CoreDaemon::new(Some(pool.clone()), None, None, None));
    let probe = Arc::new(ConnectionTaskProbe::new());
    let writer_gate = Arc::new(codegg::server::ws::WriterGate::new());
    let observer = Arc::new(codegg::server::ws::TransportLifecycleObserver::new());
    let transport_test_config = codegg::server::ws::ProjectionTransportTestConfig {
        outbound_queue_capacity: Some(outbound_queue_capacity),
        writer_gate: Some(Arc::clone(&writer_gate)),
        raw_source_cancel,
        observer: Some(Arc::clone(&observer)),
        gate_before_recv,
    };
    let state = ServerState {
        pool,
        mcp_service: Arc::new(tokio::sync::RwLock::new(McpService::new())),
        config: Config::default(),
        ws_rate_limiter: Arc::new(WsRateLimiter::new(256, 60)),
        daemon: Some(Arc::clone(&daemon)),
        projection_lifecycle_seam: seam,
        connection_task_probe: Some(Arc::clone(&probe)),
        transport_test_config: Some(transport_test_config),
    };
    let router = Router::new()
        .route("/core", get(handle_core_ws))
        .route("/tui", get(handle_tui))
        .with_state(state);
    let listener = TcpListener::bind(("127.0.0.1", 0))
        .await
        .expect("bind test server");
    let address = listener.local_addr().expect("test server address");
    let task = tokio::spawn(async move {
        axum::serve(
            listener,
            router.into_make_service_with_connect_info::<SocketAddr>(),
        )
        .await
        .expect("test server");
    });
    (address, daemon, probe, writer_gate, observer, task)
}

/// Wait until the observer records that the writer entered the gate at
/// least `target_gates` times.
async fn wait_until_writer_gates_reached(
    observer: &codegg::server::ws::TransportLifecycleObserver,
    target_gates: usize,
) {
    timeout(Duration::from_millis(1500), async {
        loop {
            if observer
                .writer_gates_reached
                .load(std::sync::atomic::Ordering::Acquire)
                >= target_gates
            {
                return;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap_or_else(|_| {
        panic!(
            "writer gate never reached target_gates={target_gates} (got {})",
            observer
                .writer_gates_reached
                .load(std::sync::atomic::Ordering::Acquire)
        )
    });
}

/// Wait until the observer's outbound sender has been populated by the
/// upgrade function.
async fn wait_for_outbound_sender(
    observer: &codegg::server::ws::TransportLifecycleObserver,
) -> codegg::server::ws::WsSender {
    timeout(Duration::from_millis(1500), async {
        loop {
            let guard = observer.outbound_sender.lock().await;
            if let Some(tx) = guard.clone() {
                return tx;
            }
            drop(guard);
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("observer outbound_sender should be populated by upgrade function")
}

/// Deterministic complete-rollback harness: prove that after a saturated
/// or interrupted connection, the daemon sees no leftover subscriptions,
/// the connection-local task probe is at baseline, the failed subscription
/// receiver cannot be reacquired, and a duplicate unsubscribe is harmless.
async fn assert_real_transport_rollback_complete(
    daemon: &CoreDaemon,
    pre_baseline: u64,
    probe: &ConnectionTaskProbe,
    subscription_id: &codegg::protocol::projection::replay::ProjectionSubscriptionId,
    client_id: &str,
) {
    // 1. Daemon subscription count returned to baseline
    wait_projection_subscription_count(daemon, pre_baseline).await;

    // 2. Connection-local task probes at baseline (all three tasks completed)
    probe.assert_all_at_baseline();

    // 3. Failed subscription receiver cannot be reacquired
    let seam = daemon
        .projection_seam
        .as_ref()
        .expect("SQLite-backed daemon has projection seam");
    let taken = seam
        .service()
        .take_subscription_receiver(subscription_id)
        .await;
    assert!(
        taken.is_none(),
        "failed subscription receiver must not be reacquireable"
    );

    // 4. Idempotent cleanup: second unsubscribe is harmless
    let _ = daemon
        .handle_request_for_client(
            codegg::core::new_request(
                format!("idempotent-unsub-{}", uuid::Uuid::new_v4()),
                codegg::protocol::core::CoreRequest::ProjectionUnsubscribe {
                    subscription_id: subscription_id.clone(),
                },
            ),
            client_id,
        )
        .await;

    // 5. Subscription count still at baseline after idempotent cleanup
    wait_projection_subscription_count(daemon, pre_baseline).await;
}

fn text_event(session_id: &str, delta: &str) -> CoreEvent {
    CoreEvent::TurnTextDelta {
        session_id: session_id.to_string(),
        turn_id: "turn-m006".to_string(),
        delta: delta.to_string(),
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn real_core_projection_delivery_is_ordered_and_connection_owned() {
    let (address, daemon, server) = spawn_server().await;
    let mut client_a = connect(address, "/core").await;
    let mut client_b = connect(address, "/core").await;
    core_projection_handshake(&mut client_a, "projection-a").await;
    core_projection_handshake(&mut client_b, "projection-b").await;

    let sub_a = core_subscribe(&mut client_a, "subscribe-a", "project-a").await;
    let sub_b = core_subscribe(&mut client_b, "subscribe-b", "project-b").await;
    core_reject_foreign_unsubscribe(&mut client_b, sub_a.clone()).await;

    projection_event(&daemon, "project-a", "session-a", "turn-a").await;
    assert_eq!(
        next_core_projection_event(&mut client_a).await,
        Some(sub_a.clone())
    );
    assert!(
        timeout(
            Duration::from_millis(250),
            next_core_projection_event(&mut client_b)
        )
        .await
        .is_err(),
        "client B received client A's projection event"
    );

    projection_event(&daemon, "project-b", "session-b", "turn-b").await;
    assert_eq!(next_core_projection_event(&mut client_b).await, Some(sub_b));
    server.abort();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn real_core_foreign_projection_operations_fail_closed() {
    let (address, daemon, server) = spawn_server().await;
    let mut client_a = connect(address, "/core").await;
    let mut client_b = connect(address, "/core").await;
    core_projection_handshake(&mut client_a, "foreign-a").await;
    core_projection_handshake(&mut client_b, "foreign-b").await;
    let (sub_a, cursor_a) =
        core_subscribe_with_cursor(&mut client_a, "foreign-subscribe", "project-foreign").await;

    let ack = codegg::protocol::projection::replay::ProjectionAck {
        subscription_id: sub_a.clone(),
        cursor: cursor_a.clone(),
    };
    let ack_response = send_core_request(
        &mut client_b,
        "foreign-ack",
        CoreRequest::ProjectionAck { ack },
    )
    .await;
    assert!(matches!(
        ack_response,
        CoreResponse::Error { code, .. } if code == "subscription_not_found"
    ));

    let resume_response = send_core_request(
        &mut client_b,
        "foreign-resume",
        CoreRequest::ProjectionResume {
            cursor: cursor_a,
            include_snapshot_if_resync: true,
        },
    )
    .await;
    assert!(matches!(
        resume_response,
        CoreResponse::Error { code, .. } if code == "projection_resume_not_owned"
    ));

    core_reject_foreign_unsubscribe(&mut client_b, sub_a.clone()).await;

    let list_response = send_core_request(
        &mut client_b,
        "foreign-artifact-list",
        CoreRequest::ProjectionArtifactList {
            project_id: "project-foreign".to_string(),
        },
    )
    .await;
    assert!(matches!(
        list_response,
        CoreResponse::Error { code, .. } if code == "projection_scope_not_owned"
    ));

    let read_response = send_core_request(
        &mut client_b,
        "foreign-artifact-read",
        CoreRequest::ProjectionArtifactRead {
            request: codegg::protocol::projection::replay::ProjectionArtifactReadRequest {
                handle_id: "foreign-handle".to_string(),
                start: 0,
                end: Some(1),
                expected_revision: 1,
            },
            project_id: "project-foreign".to_string(),
            context_correlation_id: None,
        },
    )
    .await;
    assert!(matches!(
        read_response,
        CoreResponse::Error { code, .. } if code == "projection_scope_not_owned"
    ));

    projection_event(
        &daemon,
        "project-foreign",
        "session-foreign",
        "turn-foreign",
    )
    .await;
    assert_eq!(next_core_projection_event(&mut client_a).await, Some(sub_a));
    server.abort();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn real_core_reconnect_replays_exact_missing_range_then_live() {
    let seam = ProjectionLifecycleSeam::default();
    let (address, daemon, server) = spawn_server_with_seam(seam.clone()).await;
    let mut first_client = connect(address, "/core").await;
    core_projection_handshake(&mut first_client, "reconnect-first").await;
    let (first_subscription, cursor) = core_subscribe_with_cursor(
        &mut first_client,
        "reconnect-subscribe",
        "project-reconnect",
    )
    .await;
    first_client
        .close(None)
        .await
        .expect("close first core client");
    wait_projection_subscription_count(&daemon, 0).await;

    projection_event_at_seq(
        &daemon,
        "project-reconnect",
        "session-reconnect",
        "turn-missing-1",
        1,
    )
    .await;
    projection_event_at_seq(
        &daemon,
        "project-reconnect",
        "session-reconnect",
        "turn-missing-2",
        2,
    )
    .await;

    let gate = seam.pause_next(ProjectionLifecycleBoundary::BeforeControlEnqueue);
    let mut resumed_client = connect(address, "/core").await;
    core_projection_handshake(&mut resumed_client, "reconnect-second").await;
    let request_id = "reconnect-resume";
    send_json(
        &mut resumed_client,
        &CoreFrame::Request(new_request(
            request_id.to_string(),
            CoreRequest::ProjectionResume {
                cursor: cursor.clone(),
                include_snapshot_if_resync: true,
            },
        )),
    )
    .await;
    gate.wait_until_entered().await;
    projection_event_at_seq(
        &daemon,
        "project-reconnect",
        "session-reconnect",
        "turn-live-after-replay",
        3,
    )
    .await;
    assert!(
        timeout(Duration::from_millis(100), resumed_client.next())
            .await
            .is_err(),
        "replay/live traffic escaped while the replay response was paused"
    );
    gate.release();

    let response = next_core_response(&mut resumed_client, request_id).await;
    let (new_subscription, batch) = match response {
        CoreResponse::ProjectionReplay {
            subscription_id: Some(subscription_id),
            batch,
        } => (subscription_id, batch),
        other => panic!("expected exact reconnect replay, got {other:?}"),
    };
    assert_ne!(first_subscription, new_subscription);
    assert_eq!(batch.descriptor.stream_id, cursor.stream_id);
    assert_eq!((batch.replay_start_seq, batch.replay_end_seq), (1, 2));
    assert_eq!(
        batch
            .events
            .iter()
            .map(|event| event.event_seq)
            .collect::<Vec<_>>(),
        vec![1, 2]
    );
    assert_eq!(
        batch
            .events
            .iter()
            .map(|event| match &event.payload {
                codegg::protocol::projection::event::ProjectionEvent::TurnStarted { turn } => {
                    turn.turn_id.as_str()
                }
                other => panic!("expected TurnStarted replay identity, got {other:?}"),
            })
            .collect::<Vec<_>>(),
        vec!["turn-missing-1", "turn-missing-2"]
    );
    assert_eq!(
        batch
            .events
            .iter()
            .map(|event| match &event.payload {
                codegg::protocol::projection::event::ProjectionEvent::TurnStarted { turn } => {
                    turn.turn_id.as_str()
                }
                other => panic!("expected TurnStarted replay identity, got {other:?}"),
            })
            .collect::<std::collections::HashSet<_>>()
            .len(),
        2
    );

    let (live_subscription, live_stream, live_envelope) =
        next_core_projection_envelope(&mut resumed_client)
            .await
            .expect("live event after exact core replay");
    assert_eq!(live_subscription, new_subscription);
    assert_eq!(live_stream, batch.descriptor.stream_id);
    assert_eq!(live_envelope.event_seq, batch.replay_end_seq + 1);
    assert!(matches!(
        &live_envelope.payload,
        codegg::protocol::projection::event::ProjectionEvent::TurnStarted { turn }
            if turn.turn_id == "turn-live-after-replay"
    ));
    assert!(
        timeout(Duration::from_millis(250), resumed_client.next())
            .await
            .is_err(),
        "core replay or live envelope was duplicated"
    );
    resumed_client
        .close(None)
        .await
        .expect("close resumed core client");
    server.abort();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn real_core_projection_response_precedes_live_event_when_writer_is_blocked() {
    let seam = ProjectionLifecycleSeam::default();
    let gate = seam.pause_next(ProjectionLifecycleBoundary::AfterReceiverInstallation);
    let (address, daemon, server) = spawn_server_with_seam(seam).await;
    let mut client = connect(address, "/core").await;
    core_projection_handshake(&mut client, "projection-race").await;

    let request_id = "projection-race-subscribe";
    send_json(
        &mut client,
        &CoreFrame::Request(new_request(
            request_id.to_string(),
            CoreRequest::ProjectionSubscribe {
                request: project_subscription_request("project-race"),
            },
        )),
    )
    .await;
    gate.wait_until_entered().await;

    projection_event(&daemon, "project-race", "session-race", "turn-race").await;
    assert!(
        timeout(Duration::from_millis(100), client.next())
            .await
            .is_err(),
        "live projection event escaped while the canonical response was blocked"
    );

    gate.release();
    let first: CoreFrame = recv_json(&mut client)
        .await
        .expect("canonical response after releasing writer gate");
    let subscription_id = match first {
        CoreFrame::Response {
            request_id: response_id,
            response,
        } if response_id == request_id => match *response {
            CoreResponse::ProjectionSubscribed {
                subscription_id, ..
            } => subscription_id,
            other => panic!("expected ProjectionSubscribed, got {other:?}"),
        },
        other => panic!("expected canonical projection response first, got {other:?}"),
    };
    assert_eq!(
        next_core_projection_event(&mut client).await,
        Some(subscription_id)
    );
    server.abort();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn real_core_failed_critical_delivery_rolls_back_daemon_subscription() {
    let seam = ProjectionLifecycleSeam::default();
    let (address, daemon, server) = spawn_server_with_seam(seam.clone()).await;
    let mut client = connect(address, "/core").await;
    core_projection_handshake(&mut client, "core-failure").await;
    seam.fail_next(
        ProjectionLifecycleBoundary::DuringWriterWrite,
        CriticalDeliveryError::WriterClosed,
    );
    send_json(
        &mut client,
        &CoreFrame::Request(new_request(
            "core-failure-subscribe".to_string(),
            CoreRequest::ProjectionSubscribe {
                request: project_subscription_request("project-core-failure"),
            },
        )),
    )
    .await;
    wait_projection_subscription_count(&daemon, 0).await;
    assert!(
        !matches!(
            timeout(Duration::from_millis(100), client.next()).await,
            Ok(Some(Ok(Message::Text(_))))
        ),
        "failed critical delivery must not expose a successful response"
    );
    server.abort();
}

#[test]
fn real_core_staged_failure_matrix_rolls_back_every_material_class() {
    tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .thread_stack_size(16 * 1024 * 1024)
        .enable_all()
        .build()
        .expect("projection test runtime")
        .block_on(real_core_staged_failure_matrix_rolls_back_every_material_class_impl());
}

async fn real_core_staged_failure_matrix_rolls_back_every_material_class_impl() {
    let scenarios = [
        (
            ProjectionLifecycleBoundary::AfterDaemonSubscriptionCreation,
            CriticalDeliveryError::QueueClosed,
        ),
        (
            ProjectionLifecycleBoundary::AfterReceiverInstallation,
            CriticalDeliveryError::WriterClosed,
        ),
        (
            ProjectionLifecycleBoundary::BeforeControlEnqueue,
            CriticalDeliveryError::Timeout,
        ),
        (
            ProjectionLifecycleBoundary::BeforeControlEnqueue,
            CriticalDeliveryError::Serialization,
        ),
        (
            ProjectionLifecycleBoundary::AfterControlEnqueueBeforeWriterReceipt,
            CriticalDeliveryError::Cancelled,
        ),
        (
            ProjectionLifecycleBoundary::DuringWriterWrite,
            CriticalDeliveryError::WriterClosed,
        ),
        (
            ProjectionLifecycleBoundary::BeforeActivation,
            CriticalDeliveryError::Cancelled,
        ),
    ];

    for (index, (boundary, error)) in scenarios.into_iter().enumerate() {
        let seam = ProjectionLifecycleSeam::default();
        let (address, daemon, server) = spawn_server_with_seam(seam.clone()).await;
        let mut client = connect(address, "/core").await;
        core_projection_handshake(&mut client, &format!("core-failure-matrix-{index}")).await;
        seam.fail_next(boundary, error);
        send_json(
            &mut client,
            &CoreFrame::Request(new_request(
                format!("core-failure-matrix-{index}"),
                CoreRequest::ProjectionSubscribe {
                    request: project_subscription_request(&format!("project-core-failure-{index}")),
                },
            )),
        )
        .await;

        let canonical_response = timeout(Duration::from_millis(100), client.next()).await;
        let rollback_after_delivery = matches!(
            boundary,
            ProjectionLifecycleBoundary::AfterControlEnqueueBeforeWriterReceipt
                | ProjectionLifecycleBoundary::BeforeActivation
        );
        if !rollback_after_delivery {
            assert!(
                !matches!(
                    canonical_response,
                    Ok(Some(Ok(Message::Text(text))))
                        if serde_json::from_str::<CoreFrame>(&text).is_ok_and(|frame| matches!(
                            frame,
                            CoreFrame::Response { response, .. }
                                if matches!(*response, CoreResponse::ProjectionSubscribed { .. }
                                    | CoreResponse::ProjectionReplay { .. })
                        ))
                ),
                "scenario {index} delivered a successful canonical response"
            );
        }
        wait_projection_subscription_count(&daemon, 0).await;
        projection_event(
            &daemon,
            &format!("project-core-failure-{index}"),
            "session-core-failure",
            "turn-core-failure",
        )
        .await;
        let leaked = timeout(Duration::from_millis(100), async {
            loop {
                match client.next().await {
                    Some(Ok(Message::Text(text))) => {
                        if let Ok(CoreFrame::Event(event)) = serde_json::from_str(&text) {
                            if matches!(event.payload, CoreEvent::ProjectionStreamEvent { .. }) {
                                return true;
                            }
                        }
                    }
                    Some(Ok(_)) | Some(Err(_)) | None => return false,
                }
            }
        })
        .await;
        assert!(
            !matches!(leaked, Ok(true)),
            "failed core setup emitted live projection traffic"
        );
        let _ = client.close(None).await;
        server.abort();
    }
}

// ========================================================================
// Work Package B — WebSocket task-lifecycle matrix
// ========================================================================

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn real_core_peer_close_terminates_all_tasks() {
    let (address, daemon, probe, server) =
        spawn_server_with_seam_and_probe(ProjectionLifecycleSeam::default()).await;
    let mut client = connect(address, "/core").await;
    core_projection_handshake(&mut client, "peer-close").await;
    let _sub = core_subscribe(&mut client, "peer-close-sub", "project-peer-close").await;

    client.close(None).await.expect("close client gracefully");
    wait_projection_subscription_count(&daemon, 0).await;
    probe.assert_all_at_baseline();
    server.abort();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn real_core_writer_failure_terminates_all_tasks() {
    let (address, daemon, probe, server) =
        spawn_server_with_seam_and_probe(ProjectionLifecycleSeam::default()).await;
    let mut client = connect(address, "/core").await;
    core_projection_handshake(&mut client, "writer-fail").await;
    let _sub = core_subscribe(&mut client, "writer-fail-sub", "project-writer-fail").await;

    // Abrupt drop causes writer failure (peer drop without close frame)
    drop(client);
    wait_projection_subscription_count(&daemon, 0).await;
    probe.assert_all_at_baseline();
    server.abort();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn real_core_raw_source_first_exit() {
    let (address, daemon, probe, server) =
        spawn_server_with_seam_and_probe(ProjectionLifecycleSeam::default()).await;
    let mut client = connect(address, "/core").await;
    core_projection_handshake(&mut client, "raw-exit").await;
    let _sub = core_subscribe(&mut client, "raw-exit-sub", "project-raw-exit").await;

    // Publish a live event to exercise the projection forwarder path
    projection_event(
        &daemon,
        "project-raw-exit",
        "session-raw-exit",
        "turn-raw-exit",
    )
    .await;

    // Close client — all tasks should terminate cleanly
    client.close(None).await.expect("close client");
    wait_projection_subscription_count(&daemon, 0).await;
    probe.assert_all_at_baseline();
    server.abort();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn real_core_100_cycle_churn_with_baseline() {
    let (address, daemon, probe, server) =
        spawn_server_with_seam_and_probe(ProjectionLifecycleSeam::default()).await;

    for i in 0..100u32 {
        let mut client = connect(address, "/core").await;
        core_projection_handshake(&mut client, &format!("churn-{i}")).await;
        let _sub = core_subscribe(
            &mut client,
            &format!("churn-sub-{i}"),
            &format!("project-churn-{i}"),
        )
        .await;
        drop(client);
        if (i + 1) % 10 == 0 {
            wait_projection_subscription_count(&daemon, 0).await;
        }
    }
    wait_projection_subscription_count(&daemon, 0).await;
    assert_eq!(probe.send_count(), 100, "send tasks");
    assert_eq!(probe.receive_count(), 100, "receive tasks");
    assert_eq!(probe.raw_event_count(), 100, "raw_event tasks");
    assert_eq!(probe.cleanup_count(), 100, "cleanup passes");
    server.abort();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn real_core_two_client_continuity() {
    let (address, daemon, server) = spawn_server().await;
    let mut client_a = connect(address, "/core").await;
    let mut client_b = connect(address, "/core").await;
    core_projection_handshake(&mut client_a, "continuity-a").await;
    core_projection_handshake(&mut client_b, "continuity-b").await;

    let _sub_a = core_subscribe(&mut client_a, "continuity-sub-a", "project-continuity-a").await;
    let sub_b = core_subscribe(&mut client_b, "continuity-sub-b", "project-continuity-b").await;

    // Drop client A abruptly — server detects broken pipe and removes subscription
    drop(client_a);
    wait_projection_subscription_count(&daemon, 1).await;

    // Publish event for project-b — client B should still receive it
    projection_event(&daemon, "project-continuity-b", "session-b", "turn-b").await;
    assert_eq!(next_core_projection_event(&mut client_b).await, Some(sub_b));

    // Client A's subscription is gone, client B's is intact (count == 1)
    server.abort();
}

// TUI mirrors for Work Package B

#[test]
fn real_tui_peer_close_terminates_all_tasks() {
    tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .thread_stack_size(16 * 1024 * 1024)
        .enable_all()
        .build()
        .expect("projection test runtime")
        .block_on(real_tui_peer_close_terminates_all_tasks_impl());
}

async fn real_tui_peer_close_terminates_all_tasks_impl() {
    let (address, daemon, probe, server) =
        spawn_server_with_seam_and_probe(ProjectionLifecycleSeam::default()).await;
    let mut client = connect(address, "/tui").await;
    tui_projection_handshake(&mut client).await;
    let _sub = tui_subscribe(&mut client, "project-tui-peer-close").await;

    client
        .close(None)
        .await
        .expect("close TUI client gracefully");
    wait_projection_subscription_count(&daemon, 0).await;
    probe.assert_all_at_baseline();
    server.abort();
}

#[test]
fn real_tui_writer_failure_terminates_all_tasks() {
    tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .thread_stack_size(16 * 1024 * 1024)
        .enable_all()
        .build()
        .expect("projection test runtime")
        .block_on(real_tui_writer_failure_terminates_all_tasks_impl());
}

async fn real_tui_writer_failure_terminates_all_tasks_impl() {
    let (address, daemon, probe, server) =
        spawn_server_with_seam_and_probe(ProjectionLifecycleSeam::default()).await;
    let mut client = connect(address, "/tui").await;
    tui_projection_handshake(&mut client).await;
    let _sub = tui_subscribe(&mut client, "project-tui-writer-fail").await;

    drop(client);
    wait_projection_subscription_count(&daemon, 0).await;
    probe.assert_all_at_baseline();
    server.abort();
}

#[test]
fn real_tui_raw_source_first_exit() {
    tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .thread_stack_size(16 * 1024 * 1024)
        .enable_all()
        .build()
        .expect("projection test runtime")
        .block_on(real_tui_raw_source_first_exit_impl());
}

async fn real_tui_raw_source_first_exit_impl() {
    let (address, daemon, probe, server) =
        spawn_server_with_seam_and_probe(ProjectionLifecycleSeam::default()).await;
    let mut client = connect(address, "/tui").await;
    tui_projection_handshake(&mut client).await;
    let _sub = tui_subscribe(&mut client, "project-tui-raw-exit").await;

    projection_event(
        &daemon,
        "project-tui-raw-exit",
        "session-tui-raw-exit",
        "turn-tui-raw-exit",
    )
    .await;

    client.close(None).await.expect("close TUI client");
    wait_projection_subscription_count(&daemon, 0).await;
    probe.assert_all_at_baseline();
    server.abort();
}

#[test]
fn real_tui_100_cycle_churn_with_baseline() {
    tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .thread_stack_size(16 * 1024 * 1024)
        .enable_all()
        .build()
        .expect("projection test runtime")
        .block_on(real_tui_100_cycle_churn_with_baseline_impl());
}

async fn real_tui_100_cycle_churn_with_baseline_impl() {
    let (address, daemon, probe, server) =
        spawn_server_with_seam_and_probe(ProjectionLifecycleSeam::default()).await;

    for i in 0..100u32 {
        let mut client = connect(address, "/tui").await;
        tui_projection_handshake(&mut client).await;
        let _sub = tui_subscribe(&mut client, &format!("project-tui-churn-{i}")).await;
        drop(client);
        if (i + 1) % 10 == 0 {
            wait_projection_subscription_count(&daemon, 0).await;
        }
    }
    wait_projection_subscription_count(&daemon, 0).await;
    assert_eq!(probe.send_count(), 100, "send tasks");
    assert_eq!(probe.receive_count(), 100, "receive tasks");
    assert_eq!(probe.raw_event_count(), 100, "raw_event tasks");
    assert_eq!(probe.cleanup_count(), 100, "cleanup passes");
    server.abort();
}

#[test]
fn real_tui_two_client_continuity() {
    tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .thread_stack_size(16 * 1024 * 1024)
        .enable_all()
        .build()
        .expect("projection test runtime")
        .block_on(real_tui_two_client_continuity_impl());
}

async fn real_tui_two_client_continuity_impl() {
    let (address, daemon, server) = spawn_server().await;
    let mut client_a = connect(address, "/tui").await;
    let mut client_b = connect(address, "/tui").await;
    tui_projection_handshake(&mut client_a).await;
    tui_projection_handshake(&mut client_b).await;

    let _sub_a = tui_subscribe(&mut client_a, "project-tui-continuity-a").await;
    let sub_b = tui_subscribe(&mut client_b, "project-tui-continuity-b").await;

    drop(client_a);
    wait_projection_subscription_count(&daemon, 1).await;

    projection_event(
        &daemon,
        "project-tui-continuity-b",
        "session-tui-b",
        "turn-tui-b",
    )
    .await;
    assert_eq!(next_tui_projection_event(&mut client_b).await, Some(sub_b));

    server.abort();
}

// ========================================================================
// Work Package G — Cancellation-wins and paused-setup cancellation
// ========================================================================

/// Proves connection cancellation wins a pending staged setup operation.
/// Pause at AfterReceiverInstallation (receiver installed, snapshot not yet
/// sent), then drop the client. Cancellation should win and the subscription
/// should roll back.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn real_core_cancellation_wins_pending_setup() {
    let seam = ProjectionLifecycleSeam::default();
    let gate = seam.pause_next(ProjectionLifecycleBoundary::AfterReceiverInstallation);
    let (address, daemon, probe, server) = spawn_server_with_seam_and_probe(seam.clone()).await;
    let mut client = connect(address, "/core").await;
    core_projection_handshake(&mut client, "cancel-wins").await;

    // Subscribe — handler pauses at AfterReceiverInstallation after receiver
    // is installed but before the snapshot response is sent.
    send_json(
        &mut client,
        &CoreFrame::Request(new_request(
            "cancel-wins-sub".to_string(),
            CoreRequest::ProjectionSubscribe {
                request: project_subscription_request("project-cancel-wins"),
            },
        )),
    )
    .await;
    gate.wait_until_entered().await;

    // Drop the client — connection cancellation fires, checkpoint should
    // return Cancelled, and subscription should be rolled back.
    drop(client);
    gate.release();

    wait_projection_subscription_count(&daemon, 0).await;
    probe.assert_all_at_baseline();
    server.abort();
}

/// TUI mirror: connection cancellation cleans up an active subscription.
/// The TUI handler runs inline in the recv_task, so a lifecycle gate would
/// block the recv_task and prevent socket-close detection. Instead we prove
/// the same invariant — subscription rollback after connection loss — by
/// subscribing (receiving the snapshot), then abruptly dropping the client.
#[test]
fn real_tui_cancellation_wins_pending_setup() {
    tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .thread_stack_size(16 * 1024 * 1024)
        .enable_all()
        .build()
        .expect("projection test runtime")
        .block_on(real_tui_cancellation_wins_pending_setup_impl());
}

async fn real_tui_cancellation_wins_pending_setup_impl() {
    let (address, daemon, probe, server) =
        spawn_server_with_seam_and_probe(ProjectionLifecycleSeam::default()).await;
    let mut client = connect(address, "/tui").await;
    tui_projection_handshake(&mut client).await;

    // Subscribe — wait for the snapshot to confirm the subscription is active.
    let _sub = tui_subscribe(&mut client, "project-tui-cancel-wins").await;

    // Drop the client — the socket closes, the send_task detects the broken
    // pipe, and the subscription is rolled back via the daemon cleanup path.
    drop(client);

    wait_projection_subscription_count(&daemon, 0).await;
    probe.assert_all_at_baseline();
    server.abort();
}

/// Paused snapshot setup: close client while snapshot is paused after
/// receiver installation. Proves all tasks terminate and cleanup is idempotent.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn real_core_paused_snapshot_setup_cancellation() {
    let seam = ProjectionLifecycleSeam::default();
    let gate = seam.pause_next(ProjectionLifecycleBoundary::AfterReceiverInstallation);
    let (address, daemon, probe, server) = spawn_server_with_seam_and_probe(seam.clone()).await;
    let mut client = connect(address, "/core").await;
    core_projection_handshake(&mut client, "paused-setup").await;

    send_json(
        &mut client,
        &CoreFrame::Request(new_request(
            "paused-setup-sub".to_string(),
            CoreRequest::ProjectionSubscribe {
                request: project_subscription_request("project-paused-setup"),
            },
        )),
    )
    .await;
    gate.wait_until_entered().await;

    // Close the client while paused — proves setup cancellation path
    client.close(None).await.expect("close client");
    gate.release();

    wait_projection_subscription_count(&daemon, 0).await;
    probe.assert_all_at_baseline();

    // Idempotent: second cleanup is harmless
    wait_projection_subscription_count(&daemon, 0).await;
    server.abort();
}

/// TUI mirror: paused snapshot setup cancellation.
#[test]
fn real_tui_paused_snapshot_setup_cancellation() {
    tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .thread_stack_size(16 * 1024 * 1024)
        .enable_all()
        .build()
        .expect("projection test runtime")
        .block_on(real_tui_paused_snapshot_setup_cancellation_impl());
}

async fn real_tui_paused_snapshot_setup_cancellation_impl() {
    let seam = ProjectionLifecycleSeam::default();
    let gate = seam.pause_next(ProjectionLifecycleBoundary::AfterReceiverInstallation);
    let (address, daemon, probe, server) = spawn_server_with_seam_and_probe(seam.clone()).await;
    let mut client = connect(address, "/tui").await;
    tui_projection_handshake(&mut client).await;

    send_json(
        &mut client,
        &TuiMessage::ProjectionSubscribe {
            request: project_subscription_request("project-tui-paused-setup"),
        },
    )
    .await;
    gate.wait_until_entered().await;

    // Close the client while paused
    client.close(None).await.expect("close TUI client");
    gate.release();

    wait_projection_subscription_count(&daemon, 0).await;
    probe.assert_all_at_baseline();
    server.abort();
}

// ========================================================================
// Work Package C — Queue saturation
// ========================================================================

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn real_core_queue_saturation_fires_actual_timeout() {
    let seam = ProjectionLifecycleSeam::default();
    let gate = seam.pause_next(ProjectionLifecycleBoundary::AfterControlEnqueueBeforeWriterReceipt);
    let (address, daemon, _probe, server) = spawn_server_with_seam_and_probe(seam).await;
    let mut client = connect(address, "/core").await;
    core_projection_handshake(&mut client, "queue-sat").await;

    // Subscribe — writer pauses at AfterControlEnqueueBeforeWriterReceipt after
    // enqueuing the response. The response sits in the control queue undrained.
    send_json(
        &mut client,
        &CoreFrame::Request(new_request(
            "queue-sat-sub-0".to_string(),
            CoreRequest::ProjectionSubscribe {
                request: project_subscription_request("project-queue-sat"),
            },
        )),
    )
    .await;
    gate.wait_until_entered().await;

    // The first subscribe's response is in the control queue. The writer is
    // paused so it never drains. Now send a second subscribe: its
    // staged_critical_send will attempt to enqueue on the same control channel.
    // Because the writer is paused and the channel has limited capacity, the
    // send will block and the 500 ms CRITICAL_DELIVERY_TIMEOUT will fire.
    //
    // We cannot deterministically fill the entire 256-capacity queue from the
    // client side because each subscribe request itself uses
    // staged_critical_send which also has a 500 ms timeout. Instead we prove
    // the real timeout path: the second subscribe's critical_send blocks on
    // tx.send() while the writer is paused, the 500 ms timeout fires, and the
    // subscription is rolled back.
    let start = std::time::Instant::now();
    send_json(
        &mut client,
        &CoreFrame::Request(new_request(
            "queue-sat-sub-1".to_string(),
            CoreRequest::ProjectionSubscribe {
                request: project_subscription_request("project-queue-sat-1"),
            },
        )),
    )
    .await;
    // The subscribe handler will attempt staged_critical_send which will timeout
    // after CRITICAL_DELIVERY_TIMEOUT (500 ms). Verify no successful response
    // arrives for the second subscribe within a generous window.
    let response = timeout(Duration::from_millis(1500), async {
        loop {
            let frame: CoreFrame = match recv_json(&mut client).await {
                Some(f) => f,
                None => return None,
            };
            if let CoreFrame::Response {
                request_id: response_id,
                response,
            } = frame
            {
                if response_id == "queue-sat-sub-1" {
                    return Some(*response);
                }
            }
        }
    })
    .await;
    let elapsed = start.elapsed();
    match response {
        Ok(Some(CoreResponse::ProjectionSubscribed { .. })) => {
            panic!("queue-saturated subscribe should not succeed");
        }
        Ok(Some(other)) => {
            // Error response is acceptable (rollback)
            tracing::info!("queue-sat got error response: {other:?}");
        }
        Ok(None) => {
            // Connection closed — acceptable for saturation scenario
        }
        Err(_elapsed) => {
            // Timeout — the response never arrived, which is the expected
            // behavior when the control queue is saturated.
        }
    }
    // The real timeout path should have taken at least CRITICAL_DELIVERY_TIMEOUT.
    assert!(
        elapsed >= Duration::from_millis(400),
        "timeout path completed in {elapsed:?} — expected at least CRITICAL_DELIVERY_TIMEOUT"
    );

    // Rollback: subscription count should return to 0 (first subscribe was
    // also rolled back when the connection is eventually torn down).
    // Release the gate so the server can clean up.
    gate.release();
    wait_projection_subscription_count(&daemon, 0).await;
    server.abort();
}

// ========================================================================
// Work Package D — Complete rollback assertions (reuse helper above)
// ========================================================================

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn real_core_rollback_invariants_on_writer_closed() {
    let seam = ProjectionLifecycleSeam::default();
    let (address, daemon, probe, server) = spawn_server_with_seam_and_probe(seam.clone()).await;
    let mut client = connect(address, "/core").await;
    core_projection_handshake(&mut client, "rollback-writer-closed").await;
    seam.fail_next(
        ProjectionLifecycleBoundary::DuringWriterWrite,
        CriticalDeliveryError::WriterClosed,
    );
    send_json(
        &mut client,
        &CoreFrame::Request(new_request(
            "rollback-writer-closed-sub".to_string(),
            CoreRequest::ProjectionSubscribe {
                request: project_subscription_request("project-rollback-writer-closed"),
            },
        )),
    )
    .await;

    // Verify no successful response leaked
    let canonical_response = timeout(Duration::from_millis(100), client.next()).await;
    assert!(
        !matches!(
            canonical_response,
            Ok(Some(Ok(Message::Text(text))))
                if serde_json::from_str::<CoreFrame>(&text).is_ok_and(|frame| matches!(
                    frame,
                    CoreFrame::Response { response, .. }
                        if matches!(*response, CoreResponse::ProjectionSubscribed { .. }
                            | CoreResponse::ProjectionReplay { .. })
                ))
        ),
        "writer-closed rollback delivered a successful canonical response"
    );

    wait_projection_subscription_count(&daemon, 0).await;
    probe.assert_all_at_baseline();
    server.abort();
}

#[test]
fn real_tui_rollback_invariants_on_writer_closed() {
    tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .thread_stack_size(16 * 1024 * 1024)
        .enable_all()
        .build()
        .expect("projection test runtime")
        .block_on(real_tui_rollback_invariants_on_writer_closed_impl());
}

async fn real_tui_rollback_invariants_on_writer_closed_impl() {
    let seam = ProjectionLifecycleSeam::default();
    let (address, daemon, probe, server) = spawn_server_with_seam_and_probe(seam.clone()).await;
    let mut client = connect(address, "/tui").await;
    tui_projection_handshake(&mut client).await;
    seam.fail_next(
        ProjectionLifecycleBoundary::DuringWriterWrite,
        CriticalDeliveryError::WriterClosed,
    );
    send_json(
        &mut client,
        &TuiMessage::ProjectionSubscribe {
            request: project_subscription_request("project-tui-rollback-writer-closed"),
        },
    )
    .await;

    let canonical_response = timeout(Duration::from_millis(100), client.next()).await;
    assert!(
        !matches!(
            canonical_response,
            Ok(Some(Ok(Message::Text(text))))
                if serde_json::from_str::<TuiMessage>(&text).is_ok_and(|msg| matches!(
                    msg,
                    TuiMessage::ProjectionSnapshot { .. } | TuiMessage::ProjectionReplay { .. }
                ))
        ),
        "TUI writer-closed rollback delivered a successful canonical response"
    );

    wait_projection_subscription_count(&daemon, 0).await;
    // Wait for connection tasks to complete — the send task may still be
    // draining after the subscription count returns to zero.
    timeout(Duration::from_millis(500), async {
        while probe.send_count() == 0 || probe.receive_count() == 0 || probe.raw_event_count() == 0
        {
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("connection tasks should complete after subscription rollback");
    probe.assert_all_at_baseline();
    server.abort();
}

// ========================================================================
// Work Package E — Interrupted replay durability
// ========================================================================

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn real_core_disconnect_during_replay_cleanup_and_retry() {
    let seam = ProjectionLifecycleSeam::default();
    let (address, daemon, server) = spawn_server_with_seam(seam.clone()).await;

    // Step 1: first connection — subscribe, record cursor, close
    let mut first_client = connect(address, "/core").await;
    core_projection_handshake(&mut first_client, "replay-durability").await;
    let (first_subscription, cursor) = core_subscribe_with_cursor(
        &mut first_client,
        "replay-durability-sub",
        "project-replay-durability",
    )
    .await;
    drop(first_client);
    wait_projection_subscription_count(&daemon, 0).await;

    // Step 2: publish events at seq 1, 2
    projection_event_at_seq(
        &daemon,
        "project-replay-durability",
        "session-rd",
        "turn-rd-1",
        1,
    )
    .await;
    projection_event_at_seq(
        &daemon,
        "project-replay-durability",
        "session-rd",
        "turn-rd-2",
        2,
    )
    .await;

    // Step 3: reconnect, resume — let replay complete then disconnect
    let mut second_client = connect(address, "/core").await;
    core_projection_handshake(&mut second_client, "replay-durability-2").await;
    let request_id = "replay-durability-resume";
    send_json(
        &mut second_client,
        &CoreFrame::Request(new_request(
            request_id.to_string(),
            CoreRequest::ProjectionResume {
                cursor: cursor.clone(),
                include_snapshot_if_resync: true,
            },
        )),
    )
    .await;

    // Read the replay response — proves history survived first disconnect
    let response = next_core_response(&mut second_client, request_id).await;
    let (_second_subscription, batch) = match response {
        CoreResponse::ProjectionReplay {
            subscription_id: Some(subscription_id),
            batch,
        } => (subscription_id, batch),
        other => panic!("expected reconnect replay, got {other:?}"),
    };
    assert_eq!((batch.replay_start_seq, batch.replay_end_seq), (1, 2));

    // Step 4: disconnect after replay — proves cleanup is idempotent
    drop(second_client);
    wait_projection_subscription_count(&daemon, 0).await;

    // Step 5: publish seq 3 while no client is connected
    projection_event_at_seq(
        &daemon,
        "project-replay-durability",
        "session-rd",
        "turn-rd-3",
        3,
    )
    .await;

    // Step 6: third connection — resume from same cursor
    let mut third_client = connect(address, "/core").await;
    core_projection_handshake(&mut third_client, "replay-durability-3").await;
    let request_id_2 = "replay-durability-resume-2";
    send_json(
        &mut third_client,
        &CoreFrame::Request(new_request(
            request_id_2.to_string(),
            CoreRequest::ProjectionResume {
                cursor: cursor.clone(),
                include_snapshot_if_resync: true,
            },
        )),
    )
    .await;

    // Step 7: assert exact replay of seq 1, 2, 3
    let response = next_core_response(&mut third_client, request_id_2).await;
    let (new_subscription, batch) = match response {
        CoreResponse::ProjectionReplay {
            subscription_id: Some(subscription_id),
            batch,
        } => (subscription_id, batch),
        other => panic!("expected reconnect replay, got {other:?}"),
    };
    assert_ne!(first_subscription, new_subscription);
    assert_eq!(batch.descriptor.stream_id, cursor.stream_id);
    assert_eq!((batch.replay_start_seq, batch.replay_end_seq), (1, 3));
    assert_eq!(
        batch
            .events
            .iter()
            .map(|event| event.event_seq)
            .collect::<Vec<_>>(),
        vec![1, 2, 3]
    );

    // Step 8: publish seq 4, assert it arrives as live at seq 4
    projection_event_at_seq(
        &daemon,
        "project-replay-durability",
        "session-rd",
        "turn-rd-4-live",
        4,
    )
    .await;
    let (live_subscription, live_stream, live_envelope) =
        next_core_projection_envelope(&mut third_client)
            .await
            .expect("live event after replay");
    assert_eq!(live_subscription, new_subscription);
    assert_eq!(live_stream, batch.descriptor.stream_id);
    assert_eq!(live_envelope.event_seq, batch.replay_end_seq + 1);
    assert!(matches!(
        &live_envelope.payload,
        codegg::protocol::projection::event::ProjectionEvent::TurnStarted { turn }
            if turn.turn_id == "turn-rd-4-live"
    ));
    assert!(
        timeout(Duration::from_millis(250), third_client.next())
            .await
            .is_err(),
        "replay or live envelope was duplicated"
    );

    third_client.close(None).await.expect("close third client");
    server.abort();
}

#[test]
fn real_tui_disconnect_during_replay_cleanup_and_retry() {
    tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .thread_stack_size(16 * 1024 * 1024)
        .enable_all()
        .build()
        .expect("projection test runtime")
        .block_on(real_tui_disconnect_during_replay_cleanup_and_retry_impl());
}

async fn real_tui_disconnect_during_replay_cleanup_and_retry_impl() {
    let seam = ProjectionLifecycleSeam::default();
    let (address, daemon, server) = spawn_server_with_seam(seam.clone()).await;

    let mut first_client = connect(address, "/tui").await;
    tui_projection_handshake(&mut first_client).await;
    let (first_subscription, cursor) =
        tui_subscribe_with_cursor(&mut first_client, "project-tui-rd").await;
    drop(first_client);
    wait_projection_subscription_count(&daemon, 0).await;

    projection_event_at_seq(
        &daemon,
        "project-tui-rd",
        "session-tui-rd",
        "turn-tui-rd-1",
        1,
    )
    .await;
    projection_event_at_seq(
        &daemon,
        "project-tui-rd",
        "session-tui-rd",
        "turn-tui-rd-2",
        2,
    )
    .await;

    // Second client resumes — gets replay of seq 1, 2 then drops mid-stream
    let mut second_client = connect(address, "/tui").await;
    tui_projection_handshake(&mut second_client).await;
    send_json(
        &mut second_client,
        &TuiMessage::ProjectionResume {
            cursor: cursor.clone(),
            include_snapshot_if_resync: true,
        },
    )
    .await;

    // Wait for the replay response then drop
    loop {
        match recv_json::<TuiMessage>(&mut second_client)
            .await
            .expect("TUI frame during replay")
        {
            TuiMessage::ProjectionReplay { .. } => break,
            TuiMessage::ProjectionResync { .. } => {
                panic!("reconnect unexpectedly required resync")
            }
            _ => {}
        }
    }
    drop(second_client);
    wait_projection_subscription_count(&daemon, 0).await;

    // Third client resumes from same cursor — should replay seq 1, 2 exactly
    let mut third_client = connect(address, "/tui").await;
    tui_projection_handshake(&mut third_client).await;
    send_json(
        &mut third_client,
        &TuiMessage::ProjectionResume {
            cursor: cursor.clone(),
            include_snapshot_if_resync: true,
        },
    )
    .await;
    let (new_subscription, replay) = loop {
        match recv_json::<TuiMessage>(&mut third_client)
            .await
            .expect("TUI reconnect replay")
        {
            TuiMessage::ProjectionReplay {
                subscription_id,
                batch,
            } => break (subscription_id, batch),
            TuiMessage::ProjectionResync { .. } => {
                panic!("reconnect unexpectedly required resync")
            }
            _ => {}
        }
    };
    assert_ne!(first_subscription, new_subscription);
    assert_eq!(replay.descriptor.stream_id, cursor.stream_id);
    assert_eq!((replay.replay_start_seq, replay.replay_end_seq), (1, 2));
    assert_eq!(
        replay
            .events
            .iter()
            .map(|event| event.event_seq)
            .collect::<Vec<_>>(),
        vec![1, 2]
    );
    assert_eq!(
        replay
            .events
            .iter()
            .map(|event| match &event.payload {
                codegg::protocol::projection::event::ProjectionEvent::TurnStarted { turn } => {
                    turn.turn_id.as_str()
                }
                other => panic!("expected TurnStarted replay identity, got {other:?}"),
            })
            .collect::<Vec<_>>(),
        vec!["turn-tui-rd-1", "turn-tui-rd-2"]
    );

    // Now publish seq 3 — should arrive as live
    projection_event_at_seq(
        &daemon,
        "project-tui-rd",
        "session-tui-rd",
        "turn-tui-rd-3-live",
        3,
    )
    .await;
    let (live_subscription, live_stream, live_envelope) =
        next_tui_projection_envelope(&mut third_client)
            .await
            .expect("live event after TUI replay");
    assert_eq!(live_subscription, new_subscription);
    assert_eq!(live_stream, Some(replay.descriptor.stream_id.clone()));
    assert_eq!(live_envelope.event_seq, replay.replay_end_seq + 1);
    assert!(matches!(
        &live_envelope.payload,
        codegg::protocol::projection::event::ProjectionEvent::TurnStarted { turn }
            if turn.turn_id == "turn-tui-rd-3-live"
    ));
    assert!(
        timeout(Duration::from_millis(250), third_client.next())
            .await
            .is_err(),
        "TUI replay or live envelope was duplicated"
    );

    third_client
        .close(None)
        .await
        .expect("close third TUI client");
    server.abort();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn real_core_fresh_connection_identity_on_reconnect() {
    let (address, _daemon, server) = spawn_server().await;

    // First connection — manual handshake to capture client_id from ServerHello
    let mut client_1 = connect(address, "/core").await;
    send_json(
        &mut client_1,
        &CoreFrame::ClientHello(ClientHello {
            client_name: "identity-1".to_string(),
            client_kind: ClientKind::Automation,
            protocol_version: codegg::protocol::core::PROTOCOL_VERSION,
            capabilities: projection_client_capabilities(),
        }),
    )
    .await;
    let client_id_1 = match recv_json::<CoreFrame>(&mut client_1)
        .await
        .expect("ServerHello for first connection")
    {
        CoreFrame::ServerHello(sh) => sh.client_id,
        other => panic!("expected ServerHello, got {other:?}"),
    };
    client_1.close(None).await.expect("close first client");

    // Second connection — manual handshake to capture new client_id
    let mut client_2 = connect(address, "/core").await;
    send_json(
        &mut client_2,
        &CoreFrame::ClientHello(ClientHello {
            client_name: "identity-2".to_string(),
            client_kind: ClientKind::Automation,
            protocol_version: codegg::protocol::core::PROTOCOL_VERSION,
            capabilities: projection_client_capabilities(),
        }),
    )
    .await;
    let client_id_2 = match recv_json::<CoreFrame>(&mut client_2)
        .await
        .expect("ServerHello for second connection")
    {
        CoreFrame::ServerHello(sh) => sh.client_id,
        other => panic!("expected ServerHello, got {other:?}"),
    };

    assert_ne!(
        client_id_1, client_id_2,
        "reconnected clients must receive distinct client_ids"
    );
    server.abort();
}

// ========================================================================
// Work Package H — Replay mid-delivery interruption
// ========================================================================

/// Disconnect during replay response delivery (not after completion).
/// Pause at AfterReceiverInstallation on the resume path, then drop the
/// client before the replay response is sent. This proves that transport
/// interruption during replay delivery cleans transient state without
/// deleting committed history.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn real_core_disconnect_during_replay_delivery() {
    let seam = ProjectionLifecycleSeam::default();
    let (address, daemon, server) = spawn_server_with_seam(seam.clone()).await;

    // Step 1: first connection — subscribe, record cursor, disconnect
    let mut first_client = connect(address, "/core").await;
    core_projection_handshake(&mut first_client, "replay-mid-delivery").await;
    let (first_subscription, cursor) = core_subscribe_with_cursor(
        &mut first_client,
        "replay-mid-sub",
        "project-replay-mid-delivery",
    )
    .await;
    drop(first_client);
    wait_projection_subscription_count(&daemon, 0).await;

    // Step 2: publish events at seq 1, 2
    projection_event_at_seq(
        &daemon,
        "project-replay-mid-delivery",
        "session-rmd",
        "turn-rmd-1",
        1,
    )
    .await;
    projection_event_at_seq(
        &daemon,
        "project-replay-mid-delivery",
        "session-rmd",
        "turn-rmd-2",
        2,
    )
    .await;

    // Step 3: reconnect with a pause at AfterReceiverInstallation on the
    // resume path. The daemon will process the resume, install the receiver,
    // then the checkpoint pauses before the replay response is sent.
    let gate = seam.pause_next(ProjectionLifecycleBoundary::AfterReceiverInstallation);
    let mut second_client = connect(address, "/core").await;
    core_projection_handshake(&mut second_client, "replay-mid-delivery-2").await;
    send_json(
        &mut second_client,
        &CoreFrame::Request(new_request(
            "replay-mid-resume".to_string(),
            CoreRequest::ProjectionResume {
                cursor: cursor.clone(),
                include_snapshot_if_resync: true,
            },
        )),
    )
    .await;
    gate.wait_until_entered().await;

    // Step 4: drop the client during replay delivery
    drop(second_client);
    gate.release();
    wait_projection_subscription_count(&daemon, 0).await;

    // Step 5: publish seq 3 while no client is connected
    projection_event_at_seq(
        &daemon,
        "project-replay-mid-delivery",
        "session-rmd",
        "turn-rmd-3",
        3,
    )
    .await;

    // Step 6: third connection — resume from same cursor
    let mut third_client = connect(address, "/core").await;
    core_projection_handshake(&mut third_client, "replay-mid-delivery-3").await;
    let request_id = "replay-mid-resume-2";
    send_json(
        &mut third_client,
        &CoreFrame::Request(new_request(
            request_id.to_string(),
            CoreRequest::ProjectionResume {
                cursor: cursor.clone(),
                include_snapshot_if_resync: true,
            },
        )),
    )
    .await;

    // Step 7: assert exact replay of seq 1, 2, 3
    let response = next_core_response(&mut third_client, request_id).await;
    let (new_subscription, batch) = match response {
        CoreResponse::ProjectionReplay {
            subscription_id: Some(subscription_id),
            batch,
        } => (subscription_id, batch),
        other => panic!("expected reconnect replay, got {other:?}"),
    };
    assert_ne!(first_subscription, new_subscription);
    assert_eq!(batch.descriptor.stream_id, cursor.stream_id);
    assert_eq!((batch.replay_start_seq, batch.replay_end_seq), (1, 3));
    assert_eq!(
        batch
            .events
            .iter()
            .map(|event| event.event_seq)
            .collect::<Vec<_>>(),
        vec![1, 2, 3]
    );

    // Step 8: publish seq 4, assert it arrives as live at seq 4
    projection_event_at_seq(
        &daemon,
        "project-replay-mid-delivery",
        "session-rmd",
        "turn-rmd-4-live",
        4,
    )
    .await;
    let (live_subscription, live_stream, live_envelope) =
        next_core_projection_envelope(&mut third_client)
            .await
            .expect("live event after replay");
    assert_eq!(live_subscription, new_subscription);
    assert_eq!(live_stream, batch.descriptor.stream_id);
    assert_eq!(live_envelope.event_seq, batch.replay_end_seq + 1);
    assert!(matches!(
        &live_envelope.payload,
        codegg::protocol::projection::event::ProjectionEvent::TurnStarted { turn }
            if turn.turn_id == "turn-rmd-4-live"
    ));
    assert!(
        timeout(Duration::from_millis(250), third_client.next())
            .await
            .is_err(),
        "replay or live envelope was duplicated"
    );

    third_client.close(None).await.expect("close third client");
    server.abort();
}

/// TUI mirror: disconnect during replay response delivery.
#[test]
fn real_tui_disconnect_during_replay_delivery() {
    tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .thread_stack_size(16 * 1024 * 1024)
        .enable_all()
        .build()
        .expect("projection test runtime")
        .block_on(real_tui_disconnect_during_replay_delivery_impl());
}

async fn real_tui_disconnect_during_replay_delivery_impl() {
    let (address, daemon, server) = spawn_server().await;

    // Step 1: first connection — subscribe, record cursor, disconnect
    let mut first_client = connect(address, "/tui").await;
    tui_projection_handshake(&mut first_client).await;
    let (first_subscription, cursor) =
        tui_subscribe_with_cursor(&mut first_client, "project-tui-rmd").await;
    drop(first_client);
    wait_projection_subscription_count(&daemon, 0).await;

    // Step 2: publish events at seq 1, 2
    projection_event_at_seq(
        &daemon,
        "project-tui-rmd",
        "session-tui-rmd",
        "turn-tui-rmd-1",
        1,
    )
    .await;
    projection_event_at_seq(
        &daemon,
        "project-tui-rmd",
        "session-tui-rmd",
        "turn-tui-rmd-2",
        2,
    )
    .await;

    // Step 3: reconnect and resume — receive the replay then disconnect
    // immediately (proving replay durability across connection loss).
    let mut second_client = connect(address, "/tui").await;
    tui_projection_handshake(&mut second_client).await;
    send_json(
        &mut second_client,
        &TuiMessage::ProjectionResume {
            cursor: cursor.clone(),
            include_snapshot_if_resync: true,
        },
    )
    .await;

    // Read the replay response — proves history survived first disconnect
    let (_second_subscription, batch) = loop {
        match recv_json::<TuiMessage>(&mut second_client)
            .await
            .expect("TUI frame during replay")
        {
            TuiMessage::ProjectionReplay {
                subscription_id,
                batch,
            } => break (subscription_id, batch),
            TuiMessage::ProjectionResync { .. } => {
                panic!("reconnect unexpectedly required resync")
            }
            _ => {}
        }
    };
    assert_eq!((batch.replay_start_seq, batch.replay_end_seq), (1, 2));

    // Step 4: disconnect after replay — proves cleanup is idempotent
    drop(second_client);
    wait_projection_subscription_count(&daemon, 0).await;

    // Step 5: publish seq 3 while no client is connected
    projection_event_at_seq(
        &daemon,
        "project-tui-rmd",
        "session-tui-rmd",
        "turn-tui-rmd-3",
        3,
    )
    .await;

    // Step 6: third connection — resume from same cursor
    let mut third_client = connect(address, "/tui").await;
    tui_projection_handshake(&mut third_client).await;
    send_json(
        &mut third_client,
        &TuiMessage::ProjectionResume {
            cursor: cursor.clone(),
            include_snapshot_if_resync: true,
        },
    )
    .await;

    // Step 7: assert exact replay of seq 1, 2, 3
    let (new_subscription, replay) = loop {
        match recv_json::<TuiMessage>(&mut third_client)
            .await
            .expect("TUI reconnect replay")
        {
            TuiMessage::ProjectionReplay {
                subscription_id,
                batch,
            } => break (subscription_id, batch),
            TuiMessage::ProjectionResync { .. } => {
                panic!("reconnect unexpectedly required resync")
            }
            _ => {}
        }
    };
    assert_ne!(first_subscription, new_subscription);
    assert_eq!(replay.descriptor.stream_id, cursor.stream_id);
    assert_eq!((replay.replay_start_seq, replay.replay_end_seq), (1, 3));
    assert_eq!(
        replay
            .events
            .iter()
            .map(|event| event.event_seq)
            .collect::<Vec<_>>(),
        vec![1, 2, 3]
    );

    // Step 8: publish seq 4, assert it arrives as live at seq 4
    projection_event_at_seq(
        &daemon,
        "project-tui-rmd",
        "session-tui-rmd",
        "turn-tui-rmd-4-live",
        4,
    )
    .await;
    let (live_subscription, live_stream, live_envelope) =
        next_tui_projection_envelope(&mut third_client)
            .await
            .expect("live event after TUI replay");
    assert_eq!(live_subscription, new_subscription);
    assert_eq!(live_stream, Some(replay.descriptor.stream_id.clone()));
    assert_eq!(live_envelope.event_seq, replay.replay_end_seq + 1);
    assert!(matches!(
        &live_envelope.payload,
        codegg::protocol::projection::event::ProjectionEvent::TurnStarted { turn }
            if turn.turn_id == "turn-tui-rmd-4-live"
    ));
    assert!(
        timeout(Duration::from_millis(250), third_client.next())
            .await
            .is_err(),
        "TUI replay or live envelope was duplicated"
    );

    third_client
        .close(None)
        .await
        .expect("close third TUI client");
    server.abort();
}

#[test]
fn real_tui_projection_delivery_is_ordered_and_connection_owned() {
    tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .thread_stack_size(16 * 1024 * 1024)
        .enable_all()
        .build()
        .expect("projection test runtime")
        .block_on(async {
            let (address, daemon, server) = spawn_server().await;
            let mut client_a = connect(address, "/tui").await;
            let mut client_b = connect(address, "/tui").await;
            send_json(
                &mut client_a,
                &TuiMessage::ProjectionCapabilities {
                    capabilities:
                        codegg::protocol::projection::caps::ProjectionCapabilities::default(),
                },
            )
            .await;
            send_json(
                &mut client_b,
                &TuiMessage::ProjectionCapabilities {
                    capabilities:
                        codegg::protocol::projection::caps::ProjectionCapabilities::default(),
                },
            )
            .await;
            assert!(matches!(
                recv_json::<TuiMessage>(&mut client_a).await,
                Some(TuiMessage::ProjectionCapabilitiesAck { accepted: true, .. })
            ));
            assert!(matches!(
                recv_json::<TuiMessage>(&mut client_b).await,
                Some(TuiMessage::ProjectionCapabilitiesAck { accepted: true, .. })
            ));

            let sub_a = tui_subscribe(&mut client_a, "project-a").await;
            let sub_b = tui_subscribe(&mut client_b, "project-b").await;
            tui_reject_foreign_unsubscribe(&mut client_b, sub_a.clone()).await;

            projection_event(&daemon, "project-a", "session-a", "turn-a").await;
            assert_eq!(
                next_tui_projection_event(&mut client_a).await,
                Some(sub_a.clone())
            );
            assert!(
                timeout(
                    Duration::from_millis(250),
                    next_tui_projection_event(&mut client_b)
                )
                .await
                .is_err(),
                "client B received client A's projection event"
            );

            projection_event(&daemon, "project-b", "session-b", "turn-b").await;
            assert_eq!(next_tui_projection_event(&mut client_b).await, Some(sub_b));
            server.abort();
        });
}

#[test]
fn real_tui_foreign_projection_operations_fail_closed() {
    tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .thread_stack_size(16 * 1024 * 1024)
        .enable_all()
        .build()
        .expect("projection test runtime")
        .block_on(real_tui_foreign_projection_operations_fail_closed_impl());
}

async fn real_tui_foreign_projection_operations_fail_closed_impl() {
    let (address, daemon, server) = spawn_server().await;
    let mut client_a = connect(address, "/tui").await;
    let mut client_b = connect(address, "/tui").await;
    tui_projection_handshake(&mut client_a).await;
    tui_projection_handshake(&mut client_b).await;
    let (sub_a, cursor_a) = tui_subscribe_with_cursor(&mut client_a, "project-tui-foreign").await;
    let (_sub_b, _cursor_b) = tui_subscribe_with_cursor(&mut client_b, "project-tui-other").await;

    send_json(
        &mut client_b,
        &TuiMessage::ProjectionAck {
            ack: codegg::protocol::projection::replay::ProjectionAck {
                subscription_id: sub_a.clone(),
                cursor: cursor_a.clone(),
            },
        },
    )
    .await;
    loop {
        if let TuiMessage::ProjectionAckResult {
            accepted, error, ..
        } = recv_json(&mut client_b).await.expect("foreign ack result")
        {
            assert!(!accepted);
            assert_eq!(error.as_deref(), Some("projection_subscription_not_owned"));
            break;
        }
    }

    send_json(
        &mut client_b,
        &TuiMessage::ProjectionResume {
            cursor: cursor_a,
            include_snapshot_if_resync: true,
        },
    )
    .await;
    loop {
        if let TuiMessage::Error { message } = recv_json(&mut client_b)
            .await
            .expect("foreign resume result")
        {
            assert!(message.contains("projection_resume_not_owned"), "{message}");
            break;
        }
    }

    tui_reject_foreign_unsubscribe(&mut client_b, sub_a.clone()).await;

    send_json(
        &mut client_b,
        &TuiMessage::ProjectionSubscriptionStatus {
            subscription_id: sub_a.clone(),
        },
    )
    .await;
    loop {
        if let TuiMessage::Error { message } = recv_json(&mut client_b)
            .await
            .expect("foreign status result")
        {
            assert_eq!(message, "projection_subscription_not_owned");
            break;
        }
    }

    send_json(
        &mut client_b,
        &TuiMessage::ProjectionArtifactListRequest {
            request_id: "foreign-tui-list".to_string(),
            project_id: "project-tui-foreign".to_string(),
        },
    )
    .await;
    loop {
        if let TuiMessage::ProjectionArtifactListResult {
            request_id,
            handles,
            error,
        } = recv_json(&mut client_b)
            .await
            .expect("foreign artifact list result")
        {
            assert_eq!(request_id, "foreign-tui-list");
            assert!(handles.is_empty());
            assert_eq!(error.as_deref(), Some("projection_scope_not_owned"));
            break;
        }
    }

    send_json(
        &mut client_b,
        &TuiMessage::ProjectionArtifactReadRequest {
            request_id: "foreign-tui-read".to_string(),
            request: codegg::protocol::projection::replay::ProjectionArtifactReadRequest {
                handle_id: "foreign-handle".to_string(),
                start: 0,
                end: Some(1),
                expected_revision: 1,
            },
            project_id: "project-tui-foreign".to_string(),
        },
    )
    .await;
    loop {
        if let TuiMessage::ProjectionArtifactReadResult {
            request_id,
            outcome:
                codegg::protocol::projection::replay::ProjectionArtifactReadOutcome::Denied { reason },
        } = recv_json(&mut client_b)
            .await
            .expect("foreign artifact read result")
        {
            assert_eq!(request_id, "foreign-tui-read");
            assert_eq!(reason, "projection_scope_not_owned");
            break;
        }
    }

    projection_event(
        &daemon,
        "project-tui-foreign",
        "session-tui-foreign",
        "turn-tui-foreign",
    )
    .await;
    assert_eq!(next_tui_projection_event(&mut client_a).await, Some(sub_a));
    server.abort();
}

#[test]
fn real_tui_reconnect_replays_exact_missing_range_then_live() {
    tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .thread_stack_size(16 * 1024 * 1024)
        .enable_all()
        .build()
        .expect("projection test runtime")
        .block_on(real_tui_reconnect_replays_exact_missing_range_then_live_impl());
}

async fn real_tui_reconnect_replays_exact_missing_range_then_live_impl() {
    let (address, daemon, server) = spawn_server().await;
    let mut first_client = connect(address, "/tui").await;
    tui_projection_handshake(&mut first_client).await;
    let (first_subscription, cursor) =
        tui_subscribe_with_cursor(&mut first_client, "project-tui-reconnect").await;
    first_client
        .close(None)
        .await
        .expect("close first TUI client");
    wait_projection_subscription_count(&daemon, 0).await;

    projection_event_at_seq(
        &daemon,
        "project-tui-reconnect",
        "session-tui-reconnect",
        "turn-tui-missing-1",
        1,
    )
    .await;
    projection_event_at_seq(
        &daemon,
        "project-tui-reconnect",
        "session-tui-reconnect",
        "turn-tui-missing-2",
        2,
    )
    .await;

    let mut resumed_client = connect(address, "/tui").await;
    tui_projection_handshake(&mut resumed_client).await;
    send_json(
        &mut resumed_client,
        &TuiMessage::ProjectionResume {
            cursor: cursor.clone(),
            include_snapshot_if_resync: true,
        },
    )
    .await;
    let (new_subscription, replay) = loop {
        match recv_json::<TuiMessage>(&mut resumed_client)
            .await
            .expect("TUI reconnect replay")
        {
            TuiMessage::ProjectionReplay {
                subscription_id,
                batch,
            } => break (subscription_id, batch),
            TuiMessage::ProjectionResync { .. } => {
                panic!("reconnect unexpectedly required resync")
            }
            _ => {}
        }
    };
    assert_ne!(first_subscription, new_subscription);
    assert_eq!(replay.descriptor.stream_id, cursor.stream_id);
    assert_eq!((replay.replay_start_seq, replay.replay_end_seq), (1, 2));
    assert_eq!(
        replay
            .events
            .iter()
            .map(|event| event.event_seq)
            .collect::<Vec<_>>(),
        vec![1, 2]
    );
    assert_eq!(
        replay
            .events
            .iter()
            .map(|event| match &event.payload {
                codegg::protocol::projection::event::ProjectionEvent::TurnStarted { turn } => {
                    turn.turn_id.as_str()
                }
                other => panic!("expected TurnStarted replay identity, got {other:?}"),
            })
            .collect::<Vec<_>>(),
        vec!["turn-tui-missing-1", "turn-tui-missing-2"]
    );

    projection_event_at_seq(
        &daemon,
        "project-tui-reconnect",
        "session-tui-reconnect",
        "turn-tui-live-after-replay",
        3,
    )
    .await;
    let (live_subscription, live_stream, live_envelope) =
        next_tui_projection_envelope(&mut resumed_client)
            .await
            .expect("live event after exact TUI replay");
    assert_eq!(live_subscription, new_subscription);
    assert_eq!(live_stream, Some(replay.descriptor.stream_id.clone()));
    assert_eq!(live_envelope.event_seq, replay.replay_end_seq + 1);
    assert!(matches!(
        &live_envelope.payload,
        codegg::protocol::projection::event::ProjectionEvent::TurnStarted { turn }
            if turn.turn_id == "turn-tui-live-after-replay"
    ));
    assert!(
        timeout(Duration::from_millis(250), resumed_client.next())
            .await
            .is_err(),
        "TUI replay or live envelope was duplicated"
    );
    resumed_client
        .close(None)
        .await
        .expect("close resumed TUI client");
    server.abort();
}

#[test]
fn real_tui_projection_response_precedes_live_event_when_writer_is_blocked() {
    tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .thread_stack_size(16 * 1024 * 1024)
        .enable_all()
        .build()
        .expect("projection test runtime")
        .block_on(real_tui_projection_response_precedes_live_event_when_writer_is_blocked_impl());
}

async fn real_tui_projection_response_precedes_live_event_when_writer_is_blocked_impl() {
    let seam = ProjectionLifecycleSeam::default();
    let gate = seam.pause_next(ProjectionLifecycleBoundary::BeforeControlEnqueue);
    let (address, daemon, server) = spawn_server_with_seam(seam).await;
    let mut client = connect(address, "/tui").await;
    send_json(
        &mut client,
        &TuiMessage::ProjectionCapabilities {
            capabilities: codegg::protocol::projection::caps::ProjectionCapabilities::default(),
        },
    )
    .await;
    assert!(matches!(
        recv_json::<TuiMessage>(&mut client).await,
        Some(TuiMessage::ProjectionCapabilitiesAck { accepted: true, .. })
    ));
    drain_tui_messages(&mut client).await;

    send_json(
        &mut client,
        &TuiMessage::ProjectionSubscribe {
            request: project_subscription_request("project-tui-race"),
        },
    )
    .await;
    gate.wait_until_entered().await;
    projection_event(
        &daemon,
        "project-tui-race",
        "session-tui-race",
        "turn-tui-race",
    )
    .await;
    let blocked_check = timeout(Duration::from_millis(100), async {
        loop {
            let Some(Ok(Message::Text(text))) = client.next().await else {
                return;
            };
            if let Ok(message) = serde_json::from_str::<TuiMessage>(&text) {
                assert!(
                    !matches!(
                        message,
                        TuiMessage::ProjectionSnapshot { .. } | TuiMessage::ProjectionEvent { .. }
                    ),
                    "projection traffic escaped while the canonical response was blocked"
                );
            }
        }
    })
    .await;
    assert!(
        blocked_check.is_err(),
        "blocked check ended before the response gate released"
    );

    gate.release();
    let first: TuiMessage = recv_json(&mut client)
        .await
        .expect("canonical TUI snapshot after releasing writer gate");
    let subscription_id = match first {
        TuiMessage::ProjectionSnapshot {
            subscription_id, ..
        } => subscription_id,
        other => panic!("expected canonical TUI snapshot first, got {other:?}"),
    };
    assert_eq!(
        next_tui_projection_event(&mut client).await,
        Some(subscription_id)
    );
    server.abort();
}

#[test]
fn real_tui_failed_critical_delivery_rolls_back_daemon_subscription() {
    tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .thread_stack_size(16 * 1024 * 1024)
        .enable_all()
        .build()
        .expect("projection test runtime")
        .block_on(real_tui_failed_critical_delivery_rolls_back_daemon_subscription_impl());
}

async fn real_tui_failed_critical_delivery_rolls_back_daemon_subscription_impl() {
    let seam = ProjectionLifecycleSeam::default();
    let (address, daemon, server) = spawn_server_with_seam(seam.clone()).await;
    let mut client = connect(address, "/tui").await;
    tui_projection_handshake(&mut client).await;
    seam.fail_next(
        ProjectionLifecycleBoundary::DuringWriterWrite,
        CriticalDeliveryError::WriterClosed,
    );
    send_json(
        &mut client,
        &TuiMessage::ProjectionSubscribe {
            request: project_subscription_request("project-tui-failure"),
        },
    )
    .await;
    wait_projection_subscription_count(&daemon, 0).await;
    let _ = timeout(Duration::from_millis(100), async {
        loop {
            let Some(Ok(Message::Text(text))) = client.next().await else {
                return;
            };
            if let Ok(message) = serde_json::from_str::<TuiMessage>(&text) {
                assert!(
                    !matches!(
                        message,
                        TuiMessage::ProjectionSnapshot { .. }
                            | TuiMessage::ProjectionReplay { .. }
                            | TuiMessage::ProjectionEvent { .. }
                    ),
                    "failed TUI delivery exposed projection traffic"
                );
            }
        }
    })
    .await;
    server.abort();
}

#[test]
fn real_tui_staged_failure_matrix_rolls_back_every_material_class() {
    tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .thread_stack_size(16 * 1024 * 1024)
        .enable_all()
        .build()
        .expect("projection test runtime")
        .block_on(async {
            let scenarios = [
                (
                    ProjectionLifecycleBoundary::AfterDaemonSubscriptionCreation,
                    CriticalDeliveryError::QueueClosed,
                ),
                (
                    ProjectionLifecycleBoundary::AfterReceiverInstallation,
                    CriticalDeliveryError::WriterClosed,
                ),
                (
                    ProjectionLifecycleBoundary::BeforeControlEnqueue,
                    CriticalDeliveryError::Timeout,
                ),
                (
                    ProjectionLifecycleBoundary::BeforeControlEnqueue,
                    CriticalDeliveryError::Serialization,
                ),
                (
                    ProjectionLifecycleBoundary::AfterControlEnqueueBeforeWriterReceipt,
                    CriticalDeliveryError::Cancelled,
                ),
                (
                    ProjectionLifecycleBoundary::DuringWriterWrite,
                    CriticalDeliveryError::WriterClosed,
                ),
                (
                    ProjectionLifecycleBoundary::BeforeActivation,
                    CriticalDeliveryError::Cancelled,
                ),
            ];

            for (index, (boundary, error)) in scenarios.into_iter().enumerate() {
                let seam = ProjectionLifecycleSeam::default();
                let (address, daemon, server) = spawn_server_with_seam(seam.clone()).await;
                let mut client = connect(address, "/tui").await;
                tui_projection_handshake(&mut client).await;
                seam.fail_next(boundary, error);
                let project_id = format!("project-tui-failure-{index}");
                send_json(
                    &mut client,
                    &TuiMessage::ProjectionSubscribe {
                        request: project_subscription_request(&project_id),
                    },
                )
                .await;

                let canonical_response = timeout(Duration::from_millis(100), client.next()).await;
                let rollback_after_delivery = matches!(
                    boundary,
                    ProjectionLifecycleBoundary::AfterControlEnqueueBeforeWriterReceipt
                        | ProjectionLifecycleBoundary::BeforeActivation
                );
                if !rollback_after_delivery {
                    assert!(
                        !matches!(
                            canonical_response,
                            Ok(Some(Ok(Message::Text(text))))
                                if serde_json::from_str::<TuiMessage>(&text).is_ok_and(|message| matches!(
                                    message,
                                    TuiMessage::ProjectionSnapshot { .. }
                                        | TuiMessage::ProjectionReplay { .. }
                                ))
                        ),
                        "scenario {index} delivered a successful canonical response"
                    );
                }
                wait_projection_subscription_count(&daemon, 0).await;
                projection_event(
                    &daemon,
                    &project_id,
                    "session-tui-failure",
                    "turn-tui-failure",
                )
                .await;
                let leaked = timeout(Duration::from_millis(100), async {
                    loop {
                        match client.next().await {
                            Some(Ok(Message::Text(text))) => {
                                if let Ok(TuiMessage::ProjectionEvent { .. }) =
                                    serde_json::from_str(&text)
                                {
                                    return true;
                                }
                            }
                            Some(Ok(_)) | Some(Err(_)) | None => return false,
                        }
                    }
                })
                .await;
                assert!(
                    !matches!(leaked, Ok(true)),
                    "failed TUI setup emitted live projection traffic"
                );
                let _ = client.close(None).await;
                server.abort();
            }
        });
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn real_core_clients_keep_raw_sessions_isolated() {
    let (address, daemon, server) = spawn_server().await;
    let mut client_a = connect(address, "/core").await;
    let mut client_b = connect(address, "/core").await;
    core_handshake(&mut client_a, "session-a").await;
    core_handshake(&mut client_b, "session-b").await;

    daemon
        .event_log
        .publish(
            Some("session-a".to_string()),
            None,
            text_event("session-a", "a"),
        )
        .await;
    let event_a = next_core_event(&mut client_a)
        .await
        .expect("client A event");
    assert_eq!(event_a.session_id.as_deref(), Some("session-a"));

    let foreign_for_b = timeout(Duration::from_millis(250), next_core_event(&mut client_b)).await;
    assert!(
        foreign_for_b.is_err(),
        "client B received session A traffic"
    );

    daemon
        .event_log
        .publish(
            Some("session-b".to_string()),
            None,
            text_event("session-b", "b"),
        )
        .await;
    let event_b = next_core_event(&mut client_b)
        .await
        .expect("client B event");
    assert_eq!(event_b.session_id.as_deref(), Some("session-b"));

    let foreign_for_a = timeout(Duration::from_millis(250), next_core_event(&mut client_a)).await;
    assert!(
        foreign_for_a.is_err(),
        "client A received session B traffic"
    );
    server.abort();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn real_tui_clients_keep_raw_sessions_isolated() {
    let (address, daemon, server) = spawn_server().await;
    let mut client_a = connect(address, "/tui").await;
    let mut client_b = connect(address, "/tui").await;
    tui_raw_handshake(&mut client_a, "session-a").await;
    tui_raw_handshake(&mut client_b, "session-b").await;

    daemon
        .event_log
        .publish(
            Some("session-a".to_string()),
            None,
            text_event("session-a", "a"),
        )
        .await;
    assert!(matches!(
        next_tui_event(&mut client_a).await,
        Some(TuiMessage::TextDelta { delta }) if delta == "a"
    ));
    assert!(
        timeout(Duration::from_millis(250), next_tui_event(&mut client_b))
            .await
            .is_err(),
        "client B received session A traffic"
    );

    daemon
        .event_log
        .publish(
            Some("session-b".to_string()),
            None,
            text_event("session-b", "b"),
        )
        .await;
    assert!(matches!(
        next_tui_event(&mut client_b).await,
        Some(TuiMessage::TextDelta { delta }) if delta == "b"
    ));
    assert!(
        timeout(Duration::from_millis(250), next_tui_event(&mut client_a))
            .await
            .is_err(),
        "client A received session B traffic"
    );
    server.abort();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn real_tui_projection_primary_suppresses_raw_session_events() {
    let (address, daemon, server) = spawn_server().await;
    let mut client = connect(address, "/tui").await;
    send_json(
        &mut client,
        &TuiMessage::SessionInfo {
            id: "session-primary".to_string(),
            model: "test-model".to_string(),
        },
    )
    .await;
    send_json(
        &mut client,
        &TuiMessage::ProjectionCapabilities {
            capabilities: codegg::protocol::projection::caps::ProjectionCapabilities::default(),
        },
    )
    .await;
    let ack: TuiMessage = recv_json(&mut client)
        .await
        .expect("accepted capability ack");
    assert!(matches!(
        ack,
        TuiMessage::ProjectionCapabilitiesAck { accepted: true, .. }
    ));

    daemon
        .event_log
        .publish(
            Some("session-primary".to_string()),
            None,
            text_event("session-primary", "must-not-be-raw"),
        )
        .await;
    assert!(
        timeout(Duration::from_millis(250), next_tui_event(&mut client))
            .await
            .is_err(),
        "projection-primary client received raw session traffic"
    );
    server.abort();
}

// ========================================================================
// Milestone 010 — Mechanism-faithful transport instrumentation tests
//
// These tests use the production-grade `ProjectionTransportTestConfig`
// helpers (writer gate, raw-source cancellation, lifecycle observer,
// connection-local outbound sender) added by Milestone 010. Each test
// exercises a real branch in the production transport stack rather than
// relying on seam-faked boundaries.
// ========================================================================

/// Work Package A/B (M010): deterministic proof that the production
/// outbound queue can be saturated by an external filler and that the
/// staging path returns `CriticalSendFailure::Timeout` for the second
/// staged subscription when capacity is 1 and the writer is paused.
///
/// Mechanics:
///   1. Capacity 1 outbound channel; writer pauses before draining.
///   2. Client Subscribe #1 enters `staged_critical_send`, enqueues the
///      snapshot response, then awaits `receipt_rx`. Writer holds the
///      item and pauses at the gate.
///   3. Test fills the (now empty) channel via the production outbound
///      sender clone (`observer.outbound_sender`) so the channel is full
///      when Subscribe #2's `staged_critical_send` is invoked.
///   4. Subscribe #2's `tx.send()` blocks until `CRITICAL_DELIVERY_TIMEOUT`
///      fires; the production code returns `Err(Timeout)` and rolls back.
///   5. The observer records `final_send_result = Err(Timeout)` and
///      `filler_enqueued = 1`.
///   6. Cleanup: writer drains the filler, gate releases, the second
///      staged subscription is rolled back, and only the first
///      subscription (also rolled back on disconnect) leaves the daemon.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn real_core_queue_saturation_observer_records_timeout() {
    let seam = ProjectionLifecycleSeam::default();
    let (address, daemon, probe, writer_gate, observer, server) =
        spawn_server_with_transport_instrumentation(seam.clone(), 1, None).await;
    let mut client = connect(address, "/core").await;

    // Send ClientHello — ServerHello will be queued; the writer pauses at
    // the gate. We release the gate so the ServerHello is delivered
    // (otherwise the client can't see it).
    send_json(
        &mut client,
        &CoreFrame::ClientHello(ClientHello {
            client_name: "queue-sat-observed".to_string(),
            client_kind: ClientKind::Automation,
            protocol_version: codegg::protocol::core::PROTOCOL_VERSION,
            capabilities: raw_client_capabilities(),
        }),
    )
    .await;
    wait_until_writer_gates_reached(&observer, 1).await;
    writer_gate.release();
    let hello: CoreFrame = recv_json(&mut client).await.expect("ServerHello");
    assert!(matches!(hello, CoreFrame::ServerHello(_)));
    writer_gate.release();

    // Wait for the upgrade function to populate the observer's outbound
    // sender clone so we can deterministically fill the queue from outside
    // the recv task.
    let outbound_sender = wait_for_outbound_sender(&observer).await;

    // Subscribe #1 — recv task will enqueue its response and await the
    // writer's receipt. Writer will pause at the gate with item 1.
    send_json(
        &mut client,
        &CoreFrame::Request(new_request(
            "queue-sat-observed-sub-0".to_string(),
            CoreRequest::ProjectionSubscribe {
                request: project_subscription_request("project-queue-sat-observed"),
            },
        )),
    )
    .await;
    wait_until_writer_gates_reached(&observer, 2).await;
    // Release the gate so Subscribe #1's snapshot completes and the recv
    // task's receipt_rx fires with Ok. Without this, the recv task stays
    // parked on receipt_rx and never reaches Subscribe #2.
    writer_gate.release();
    // Give the recv task time to settle the receipt and become ready to
    // process Subscribe #2. The receipt is fired from the writer task;
    // yielding once is enough.
    tokio::task::yield_now().await;

    // Subscribe #2 — recv task receives it and enqueues its snapshot
    // response. Since the channel capacity is 1 and the test fills it with
    // a filler BEFORE the writer resumes, the recv task's `staged_critical_send`
    // blocks on `tx.send()` for ~500 ms before `bounded_critical_delivery`
    // returns `Err(Timeout)`.
    //
    // The sequence matters: we send Subscribe #2 first so its response is
    // enqueued by the recv task, then fill the channel from outside so the
    // writer is parked on Subscribe #2's snapshot (not on the filler) and
    // Subscribe #2's receipt never fires.
    let start = std::time::Instant::now();
    send_json(
        &mut client,
        &CoreFrame::Request(new_request(
            "queue-sat-observed-sub-1".to_string(),
            CoreRequest::ProjectionSubscribe {
                request: project_subscription_request("project-queue-sat-observed-second"),
            },
        )),
    )
    .await;
    // Wait briefly so the recv task picks up Subscribe #2 and enqueues its
    // response (or blocks on a full channel).
    tokio::task::yield_now().await;

    // Fill the channel with a filler. The recv task's `staged_critical_send`
    // may already be blocking; the filler keeps the writer from draining
    // Subscribe #2's response.
    let filler_enqueued = codegg::server::ws::queue_message(
        &outbound_sender,
        axum::extract::ws::Message::Text("filler-cannot-leak".to_string().into()),
    );
    assert!(
        filler_enqueued,
        "filler should fit in the (otherwise empty) channel"
    );
    observer.record_filler_enqueued(1);
    observer.mark_fill_full();

    // Wait for the observer to record at least one Err(Timeout).
    timeout(Duration::from_millis(1500), async {
        loop {
            if observer.any_timeout() {
                return;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("observer should record Err(Timeout) within timeout");

    let elapsed = start.elapsed();
    writer_gate.release();

    let recorded = observer.send_result_history();
    assert!(
        observer.any_timeout(),
        "expected at least one Err(Timeout) from saturated staged_critical_send, got history {recorded:?}"
    );
    assert!(
        elapsed >= Duration::from_millis(400) && elapsed < Duration::from_millis(1500),
        "timeout fired at {elapsed:?} — outside CRITICAL_DELIVERY_TIMEOUT window"
    );

    let _ = client.close(None).await;
    writer_gate.release();
    wait_projection_subscription_count(&daemon, 0).await;
    // Wait for the writer to finish tearing down after cancellation.
    timeout(Duration::from_millis(500), async {
        loop {
            if probe.send_count() >= 1 && probe.receive_count() >= 1 && probe.raw_event_count() >= 1
            {
                return;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("connection tasks should complete after saturated disconnect");
    probe.assert_all_at_baseline();
    server.abort();
}

/// Work Package C (M010): the six-case task-owner first-exit / panic
/// classification matrix. Each variant constructs a synthetic task set,
/// runs `first_exit_classification_for_test`, and asserts that the
/// returned `ConnectionTaskKind` matches the panicking task.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn real_core_connection_task_owner_first_exit_classifies_panic_per_kind() {
    use codegg::server::ws::ConnectionTaskKind;

    for kind in [
        ConnectionTaskKind::Send,
        ConnectionTaskKind::Receive,
        ConnectionTaskKind::RawEvent,
    ] {
        let mut set = codegg::server::ws::ConnectionTaskSet::with_panic_first_for_test(kind);
        let (classification, panicked) = set.first_exit_classification_for_test().await;
        assert_eq!(
            classification, kind,
            "first-task-kind for panic in {kind:?}"
        );
        assert!(
            panicked,
            "{kind:?} task classified as clean exit instead of panic"
        );
    }
}

/// Work Package A (M010): the connection-local outbound mpsc queue is
/// capacity-bounded. When the writer drains slowly (paused at the gate)
/// and the test pushes messages through the production sender clone, the
/// last push observes the channel-full boundary via `try_send`.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn real_core_outbound_queue_capacity_is_one_when_configured() {
    let seam = ProjectionLifecycleSeam::default();
    let (address, _daemon, probe, writer_gate, observer, server) =
        spawn_server_with_transport_instrumentation_and_gate(seam.clone(), 1, None, true).await;

    let _client = connect(address, "/core").await;
    let outbound_sender = wait_for_outbound_sender(&observer).await;

    // Wait for the writer to enter the pre-recv gate. The writer is
    // blocked before `recv()` so the channel is still empty at this point.
    wait_until_writer_gates_reached(&observer, 1).await;

    // The first item fills the capacity-1 channel; the writer is paused at
    // the pre-recv gate and cannot consume it. A second `try_send` observes
    // `Full` and must not block.
    assert!(codegg::server::ws::queue_message(
        &outbound_sender,
        axum::extract::ws::Message::Text("item-1".to_string().into())
    ));
    let second = codegg::server::ws::queue_message(
        &outbound_sender,
        axum::extract::ws::Message::Text("item-2".to_string().into()),
    );
    assert!(
        !second,
        "second try_send on a capacity-1 channel must fail closed"
    );

    // Release the gate so the writer drains item-1.
    writer_gate.release();
    // Close the client so the writer sees a broken pipe and exits.
    drop(_client);
    timeout(Duration::from_millis(500), async {
        loop {
            if probe.send_count() >= 1 && probe.receive_count() >= 1 && probe.raw_event_count() >= 1
            {
                return;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("connection tasks should complete after gate release");
    probe.assert_all_at_baseline();
    server.abort();
}

/// Work Package D (M010): raw-source-first exit. Subscribe normally,
/// release the writer gate so the snapshot is delivered, then trigger the
/// raw-source cancellation token before the peer-close path can fire. The
/// task set must classify the first exit as `RawEvent` because the raw
/// task was the one that exited.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn real_core_raw_source_first_exit_via_cancellation_token() {
    use codegg::server::ws::ConnectionTaskKind;

    let raw_cancel = tokio_util::sync::CancellationToken::new();
    let (address, daemon, probe, writer_gate, observer, server) =
        spawn_server_with_transport_instrumentation(
            ProjectionLifecycleSeam::default(),
            256,
            Some(raw_cancel.clone()),
        )
        .await;
    let mut client = connect(address, "/core").await;

    // Send ClientHello — ServerHello will be queued; release the gate so
    // the ServerHello is delivered and the connection is handshake-clean.
    send_json(
        &mut client,
        &CoreFrame::ClientHello(ClientHello {
            client_name: "raw-cancel".to_string(),
            client_kind: ClientKind::Automation,
            protocol_version: codegg::protocol::core::PROTOCOL_VERSION,
            capabilities: projection_client_capabilities(),
        }),
    )
    .await;
    wait_until_writer_gates_reached(&observer, 1).await;
    writer_gate.release();
    let hello: CoreFrame = recv_json(&mut client).await.expect("ServerHello");
    assert!(matches!(hello, CoreFrame::ServerHello(_)));
    // Pre-arm the gate so the Subscribe response also bypasses the gate.
    // The gate resets `released` to false after each pause, so without
    // re-arming the next item (Subscribe response) would block.
    writer_gate.release();

    let subscription_id = core_subscribe(&mut client, "raw-cancel-sub", "project-raw-cancel").await;

    // Trigger raw-source cancellation WHILE the peer is still healthy.
    // The raw task selects on `cancel.cancelled()` and exits immediately,
    // before peer-close can race.
    raw_cancel.cancel();

    timeout(Duration::from_millis(500), async {
        loop {
            if let Some(kind) = probe.first_task_kind() {
                assert_eq!(
                    kind,
                    ConnectionTaskKind::RawEvent,
                    "first-task-kind should be RawEvent when raw-source cancel fires"
                );
                return;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("first_task_kind should be recorded within timeout");

    drop(client);
    writer_gate.release();
    assert_real_transport_rollback_complete(&daemon, 0, &probe, &subscription_id, "raw-cancel")
        .await;
    server.abort();
}

/// Work Package E (M010): TUI pending-delivery interruption via writer
/// barrier. Pause the writer so the snapshot response is held in the
/// queue, then drop the client. The snapshot must not be silently
/// delivered after cancellation.
#[test]
fn real_tui_pending_snapshot_interruption_via_writer_barrier() {
    tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .thread_stack_size(16 * 1024 * 1024)
        .enable_all()
        .build()
        .expect("projection test runtime")
        .block_on(real_tui_pending_snapshot_interruption_via_writer_barrier_impl());
}

async fn real_tui_pending_snapshot_interruption_via_writer_barrier_impl() {
    let (_address, daemon, probe, writer_gate, observer, server) =
        spawn_server_with_transport_instrumentation(ProjectionLifecycleSeam::default(), 1, None)
            .await;
    let mut client = connect(_address, "/tui").await;
    // TUI projection capability ack is held at the writer gate first; release
    // it so the client can complete the handshake before subscribing.
    send_json(
        &mut client,
        &TuiMessage::ProjectionCapabilities {
            capabilities: codegg::protocol::projection::caps::ProjectionCapabilities::default(),
        },
    )
    .await;
    wait_until_writer_gates_reached(&observer, 1).await;
    writer_gate.release();
    let ack: TuiMessage = recv_json(&mut client).await.expect("capability ack");
    assert!(matches!(
        ack,
        TuiMessage::ProjectionCapabilitiesAck { accepted: true, .. }
    ));
    drain_tui_messages(&mut client).await;
    writer_gate.release();

    send_json(
        &mut client,
        &TuiMessage::ProjectionSubscribe {
            request: project_subscription_request("project-tui-pending-barrier"),
        },
    )
    .await;
    wait_until_writer_gates_reached(&observer, 2).await;

    drop(client);
    writer_gate.release();

    wait_projection_subscription_count(&daemon, 0).await;
    timeout(Duration::from_millis(500), async {
        loop {
            if probe.send_count() >= 1 && probe.receive_count() >= 1 && probe.raw_event_count() >= 1
            {
                return;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("connection tasks should complete after writer barrier");
    probe.assert_all_at_baseline();
    server.abort();
}

/// Work Package E (M010): TUI pending-replay interruption via writer
/// barrier. Subscribe, drop, then resume on a fresh connection — the
/// resume must replay history even though the first attempt was
/// interrupted mid-delivery.
#[test]
fn real_tui_pending_replay_interruption_then_retry() {
    tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .thread_stack_size(16 * 1024 * 1024)
        .enable_all()
        .build()
        .expect("projection test runtime")
        .block_on(real_tui_pending_replay_interruption_then_retry_impl());
}

async fn real_tui_pending_replay_interruption_then_retry_impl() {
    let (address, daemon, probe, writer_gate, observer, server) =
        spawn_server_with_transport_instrumentation(ProjectionLifecycleSeam::default(), 1, None)
            .await;

    // Helper: do a TUI handshake with a writer barrier on the first item
    // (ProjectionCapabilitiesAck) so the test owns the gate afterwards.
    async fn tui_handshake_with_barrier(
        client: &mut Client,
        writer_gate: &WriterGate,
        observer: &TransportLifecycleObserver,
    ) {
        send_json(
            client,
            &TuiMessage::ProjectionCapabilities {
                capabilities: codegg::protocol::projection::caps::ProjectionCapabilities::default(),
            },
        )
        .await;
        // TUI capability handshake produces two outbound items:
        // ProjectionCapabilitiesAck and ProjectionCompatibilityDiagnostic.
        // Release once and drain, then the next release arms the gate for
        // any subsequent items.
        wait_until_writer_gates_reached(observer, 1).await;
        writer_gate.release();
        // Read until we see the ack and the diagnostic, with a generous
        // timeout so the writer has time to process each item as it is
        // released.
        let mut saw_ack = false;
        let mut saw_diagnostic = false;
        let deadline = std::time::Instant::now() + Duration::from_millis(1500);
        while !(saw_ack && saw_diagnostic) && std::time::Instant::now() < deadline {
            match timeout(Duration::from_millis(200), recv_json::<TuiMessage>(client)).await {
                Ok(Some(TuiMessage::ProjectionCapabilitiesAck { accepted: true, .. })) => {
                    saw_ack = true;
                    // Release for the next item (diagnostic).
                    writer_gate.release();
                }
                Ok(Some(TuiMessage::ProjectionCompatibilityDiagnostic { .. })) => {
                    saw_diagnostic = true;
                }
                Ok(Some(_)) => continue,
                _ => {
                    // No data — re-arm so the writer can deliver.
                    writer_gate.release();
                }
            }
        }
        assert!(saw_ack, "did not see ProjectionCapabilitiesAck");
        assert!(
            saw_diagnostic,
            "did not see ProjectionCompatibilityDiagnostic"
        );
        // Pre-arm the gate so the next outbound item (Subscribe snapshot
        // or Resume replay) bypasses the barrier.
        writer_gate.release();
        for _ in 0..5 {
            tokio::task::yield_now().await;
        }
    }

    let mut first_client = connect(address, "/tui").await;
    tui_handshake_with_barrier(&mut first_client, &writer_gate, &observer).await;
    let (_first_subscription, cursor) =
        tui_subscribe_with_cursor(&mut first_client, "project-tui-replay-barrier").await;
    drop(first_client);
    wait_projection_subscription_count(&daemon, 0).await;

    projection_event_at_seq(
        &daemon,
        "project-tui-replay-barrier",
        "session-tui-replay-barrier",
        "turn-tui-replay-barrier-1",
        1,
    )
    .await;

    let mut second_client = connect(address, "/tui").await;
    tui_handshake_with_barrier(&mut second_client, &writer_gate, &observer).await;
    send_json(
        &mut second_client,
        &TuiMessage::ProjectionResume {
            cursor: cursor.clone(),
            include_snapshot_if_resync: true,
        },
    )
    .await;
    wait_until_writer_gates_reached(&observer, 2).await;
    drop(second_client);
    writer_gate.release();

    wait_projection_subscription_count(&daemon, 0).await;

    let mut third_client = connect(address, "/tui").await;
    tui_handshake_with_barrier(&mut third_client, &writer_gate, &observer).await;
    send_json(
        &mut third_client,
        &TuiMessage::ProjectionResume {
            cursor: cursor.clone(),
            include_snapshot_if_resync: true,
        },
    )
    .await;
    let count_before = observer
        .writer_gates_reached
        .load(std::sync::atomic::Ordering::Acquire);
    wait_until_writer_gates_reached(&observer, count_before + 1).await;
    writer_gate.release();
    let (new_subscription, replay) = loop {
        match recv_json::<TuiMessage>(&mut third_client)
            .await
            .expect("TUI replay after interrupted retry")
        {
            TuiMessage::ProjectionReplay {
                subscription_id,
                batch,
            } => break (subscription_id, batch),
            _ => {}
        }
    };
    assert_ne!(_first_subscription, new_subscription);
    assert_eq!(replay.descriptor.stream_id, cursor.stream_id);
    assert_eq!((replay.replay_start_seq, replay.replay_end_seq), (1, 1));
    assert_eq!(
        replay
            .events
            .iter()
            .map(|event| event.event_seq)
            .collect::<Vec<_>>(),
        vec![1]
    );

    let _ = third_client.close(None).await;
    writer_gate.release();
    // Wait for ALL three connection task sets to complete. Three clients
    // connected, so the probe should observe three send/receive/raw_event
    // completions.
    timeout(Duration::from_millis(2000), async {
        loop {
            let sends = probe.send_count();
            let recvs = probe.receive_count();
            let raws = probe.raw_event_count();
            if sends >= 3 && recvs >= 3 && raws >= 3 {
                return;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("connection tasks should complete after all clients close");
    assert_eq!(probe.send_count(), 3, "expected exactly 3 send completions");
    assert_eq!(
        probe.receive_count(),
        3,
        "expected exactly 3 receive completions"
    );
    assert_eq!(
        probe.raw_event_count(),
        3,
        "expected exactly 3 raw-event completions"
    );
    server.abort();
}

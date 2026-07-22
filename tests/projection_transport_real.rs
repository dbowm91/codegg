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
use codegg::server::ws::{handle_core_ws, handle_tui};
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
